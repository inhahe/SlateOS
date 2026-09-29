//! The `long double` half of `<math.h>`, computed in the 80-bit format.
//!
//! Every function here computes in x87 extended precision -- 64-bit
//! significands, a 15-bit exponent -- through [`crate::ld80`]'s operations,
//! which are the x87 unit's own instructions. Before 2026-09-28 none of these
//! functions existed (a program using one did not link) and the library's
//! `long double` handling narrowed every value to a double
//! (`TD-POSIX-LONG-DOUBLE-PRECISION`).
//!
//! # Where the algorithms come from
//!
//! musl's (MIT), as the rest of `<math.h>` is (design-decisions §1132):
//! `src/math/x86_64/*.s` for the functions the x87 unit computes directly --
//! `sqrtl`, `rintl`, `floorl`, `fmodl`, `logl`, `atan2l`, `exp2l` and their
//! kin -- ported operation for operation, so each result is the one the same
//! sequence of x87 operations gives; musl's `ld80` C for the rest; and
//! FreeBSD's `fmal`, which is musl's. Where musl is wrong and glibc right,
//! this departs from musl, each time said where: `expl` for |x| >= 2^14,
//! `expm1l` for |x log2 e| > 1, and `powl` and `exp10l` wherever the result
//! is not exact -- musl's Cephes `powl` is off by up to 300 ulps for large
//! |y log2 x|, so the general case is computed afresh ("Accurate powers").
//!
//! Where C leaves an answer open, glibc's: the `errno` below, `signgam`
//! (shared with the double functions), and the exact values glibc's
//! classification functions return -- `isnanl`'s 65535, `__signbitl`'s 512.
//!
//! **`errno`** is glibc's, per function -- the rules its `w_*_template.c`
//! wrappers apply for every type alike, the same ones `math.rs` applies to
//! the double functions.
//!
//! **Every rounding direction** is handled as `math.rs`'s module
//! documentation describes for the double functions (design-decisions
//! section 1139): a range error is reported, and an overflow or underflow
//! answered, by [`crate::math::ranged`], here in the x87 unit's direction;
//! and what assumes rounding to nearest is computed to nearest
//! ([`crate::fenv::in_nearest_x87`]) -- the sine, cosine and tangent, whose
//! argument reduction's last pieces are exact only to nearest (`tanl` of
//! pi/2 less 2^-51 rounding toward zero was 7.6 million ulps out), `powl`,
//! `exp10l` and `lgammal`, which carry double-long-double values whose
//! error-free sums and products are error-free only to nearest, and
//! `tgammal`, which lost 7 ulps. Replaying glibc's directed modes
//! (`mathl_modes_oracle.txt`) found those, `expl(11357.25)` rounding downward
//! 2,502 ulps below `LDBL_MAX` where the overflow owes `LDBL_MAX` itself,
//! `log1pl(LDBL_MAX)` rounding upward an infinity (`1 + x` overflowed), and
//! the signed zeros of `math.rs`: `acoshl(1)` -0, `atanhl(1)` a NaN, and
//! `lgammal(-1)` -inf, all rounding downward.
//!
//! # The C entry points
//!
//! A `long double` argument travels in memory and a result in `%st(0)`,
//! which Rust cannot express, so each C function is an assembly thunk
//! (`crate::ld_c!`, [`crate::ld_abi`]) into an `extern "C"` Rust function
//! named `__slate_ld_<name>`, which calls the safe function of the C name
//! here. The tests call the safe functions.

// `LongDouble`'s operators are x87 floating-point operations: they round,
// overflow to infinity and propagate NaN, and cannot panic or wrap, which is
// what this lint looks for. Written out as method calls, the formulas below
// would stop reading as the formulas they are.
#![allow(clippy::arithmetic_side_effects)]
// `x - x` is C's idiom, kept from musl: a NaN made *from* the operand -- NaN
// for an infinity or a NaN, raising invalid for the infinity -- where a
// constant NaN would raise nothing and lose a NaN operand's payload.
#![allow(clippy::eq_op)]

use crate::errno;
use crate::x87::LongDouble as L;

fn set(e: i32) {
    errno::set_errno(e);
}

const ONE: L = L::ONE;
const ZERO: L = L::POS_ZERO;

/// An exact double as a long double.
fn ld(x: f64) -> L {
    L::from_f64(x)
}

/// Keep a computation whose only purpose is the flag it raises.
fn force_eval(x: L) {
    let _ = core::hint::black_box(x);
}

/// Raise invalid, as musl's `FORCE_EVAL(0/0.0f)` does.
fn raise_invalid() {
    force_eval(ZERO / core::hint::black_box(ZERO));
}

use crate::math::{Range, overflow_only, overflow_or_underflow, ranged, underflow_only};

/// `long double` for [`crate::math::ranged`]: its edges, and the x87 unit's
/// rounding direction. The x87 operations are inline assembly, which the
/// compiler never folds, so `overflow` and `underflow` round in the caller's
/// direction without a `black_box`.
impl crate::math::Real for L {
    fn at_the_edge(self) -> bool {
        // Within a binade of LDBL_MAX or past it, or below 2^-16381.
        let e = self.biased_exponent();
        !self.is_nan() && (e <= 1 || e >= 0x7FFE)
    }

    fn is_zero(self) -> bool {
        L::is_zero(self)
    }

    fn is_inf(self) -> bool {
        self.is_infinite()
    }

    fn unit_rounds_to_nearest() -> bool {
        crate::fenv::x87_rounds_to_nearest()
    }

    fn to_nearest<A>(args: A, f: impl FnOnce(A) -> Self) -> Self {
        crate::fenv::in_nearest_x87(args, f)
    }

    fn overflow(sign: L) -> L {
        let max = L::from_bits(0x7FFE, u64::MAX);
        max.copysign(sign) * max
    }

    fn underflow(sign: L) -> L {
        // 2^-10000, whose square is far below the least subnormal, 2^-16445.
        let tiny = L::from_bits(0x3FFF - 10000, 1 << 63);
        tiny.copysign(sign) * tiny
    }
}

// ===========================================================================
// Exact functions: IEEE fixes the answer
// ===========================================================================

/// `|x|`.
#[must_use]
pub fn fabsl(x: L) -> L {
    x.abs()
}

/// `|x|` with `y`'s sign.
#[must_use]
pub fn copysignl(x: L, y: L) -> L {
    x.copysign(y)
}

/// The square root, correctly rounded (`fsqrt`); `EDOM` below zero.
#[must_use]
pub fn sqrtl(x: L) -> L {
    if x < ZERO {
        set(errno::EDOM);
    }
    x.sqrt()
}

/// Round to an integer in the current direction (`frndint`).
#[must_use]
pub fn rintl(x: L) -> L {
    x.round_int()
}

/// [`rintl`] without raising inexact (musl's `nearbyintl`: note whether it
/// was set, round, and clear it again if it was not).
#[must_use]
pub fn nearbyintl(x: L) -> L {
    let had = crate::fenv::fetestexcept(crate::fenv::FE_INEXACT);
    let r = x.round_int();
    if had == 0 {
        crate::fenv::feclearexcept(crate::fenv::FE_INEXACT);
    }
    r
}

/// Round toward minus infinity.
#[must_use]
pub fn floorl(x: L) -> L {
    x.round_int_toward(1)
}

/// Round toward plus infinity.
#[must_use]
pub fn ceill(x: L) -> L {
    x.round_int_toward(2)
}

/// Round toward zero.
#[must_use]
pub fn truncl(x: L) -> L {
    x.round_int_toward(3)
}

/// `1/LDBL_EPSILON`, 2^63: adding and subtracting it rounds away every
/// fraction bit of anything below it (musl's `toint`).
const TOINT: L = L::from_bits(0x3FFF + 63, 1 << 63);

/// Round half away from zero (musl's `roundl`).
#[must_use]
pub fn roundl(x: L) -> L {
    let e = i32::from(x.biased_exponent());
    if e >= 0x3FFF + 64 - 1 {
        return x;
    }
    let neg = x.is_sign_negative();
    let a = if neg { -x } else { x };
    if e < 0x3FFF - 1 {
        force_eval(a + TOINT);
        return ZERO.copysign(x);
    }
    let mut y = a + TOINT - TOINT - a;
    y = if y > ld(0.5) {
        y + a - ONE
    } else if y <= ld(-0.5) {
        y + a + ONE
    } else {
        y + a
    };
    if neg { -y } else { y }
}

/// The nearest integer, a tie to even, whatever the rounding mode and
/// raising nothing but invalid for a signalling NaN (C23's `roundevenl`):
/// glibc's ldbl-96 `s_roundevenl.c`, on the 64-bit significand whole where
/// glibc splits it into two words.
#[must_use]
pub fn roundevenl(x: L) -> L {
    const BIAS: u16 = 0x3FFF;
    let e = x.biased_exponent();
    if e >= BIAS + 63 {
        // An integer already, an infinity, or a NaN (quieted).
        return if e == 0x7FFF { x + x } else { x };
    }
    if e >= BIAS {
        // At least 1: the bits worth 1 and 1/2 are both in the significand.
        let int_bit = 1_u64 << (BIAS + 63 - e);
        let half_bit = int_bit >> 1;
        let (mut se, mut m) = (x.sign_exp, x.significand);
        // Adding the half bit carries into the 1s bit exactly when the
        // fraction is over a half, or a half with an odd integer part --
        // the two cases that round up. With neither the integer's low bit
        // nor anything under the half bit set, the add is skipped: a half
        // over an even integer rounds down.
        if m & (int_bit | (half_bit - 1)) != 0 {
            let (sum, carry) = m.overflowing_add(half_bit);
            m = sum;
            if carry {
                m = 1 << 63;
                se += 1;
            }
        }
        return L::from_bits(se, m & !(int_bit - 1));
    }
    let sign = x.sign_exp & 0x8000;
    if e == BIAS - 1 && x.significand > 1 << 63 {
        // (0.5, 1) rounds to 1; exactly 0.5 falls through, to 0.
        return L::from_bits(sign | BIAS, 1 << 63);
    }
    L::from_bits(sign, 0)
}

/// A long double as a `long`: what the conversion instruction gives --
/// `i64::MIN` (x87's "integer indefinite") for a NaN, an infinity or a value
/// out of range, with invalid raised and no `errno`, as glibc's is.
fn to_long(r: L) -> i64 {
    r.to_i64_rint()
}

/// [`rintl`], as a `long`.
#[must_use]
pub fn lrintl(x: L) -> i64 {
    to_long(x)
}

/// [`rintl`], as a `long long`.
#[must_use]
pub fn llrintl(x: L) -> i64 {
    to_long(x)
}

/// [`roundl`], as a `long`.
#[must_use]
pub fn lroundl(x: L) -> i64 {
    to_long(roundl(x))
}

/// [`roundl`], as a `long long`.
#[must_use]
pub fn llroundl(x: L) -> i64 {
    to_long(roundl(x))
}

/// glibc's `w_fmod_template.c` and `w_remainder_template.c`: `EDOM` for an
/// infinite `x` or a zero `y`, unless either is a NaN.
fn rem_domain(x: L, y: L) {
    if (x.is_infinite() || y.is_zero()) && !x.is_nan() && !y.is_nan() {
        set(errno::EDOM);
    }
}

/// `x - n*y`, `n` truncated (`fprem`, repeated to completion).
#[must_use]
pub fn fmodl(x: L, y: L) -> L {
    rem_domain(x, y);
    L::partial_remainder::<false>(x, y).0
}

/// `x - n*y`, `n` rounded to nearest even (`fprem1`).
#[must_use]
pub fn remainderl(x: L, y: L) -> L {
    rem_domain(x, y);
    L::partial_remainder::<true>(x, y).0
}

/// [`remainderl`], and the low three bits of its quotient with the
/// quotient's sign (musl's `remquol`).
///
/// musl and glibc read those bits where `fprem1` reports them, in C0, C3 and
/// C1. This does not, since 2026-09-28: QEMU's emulated `fprem1` leaves all
/// three clear whatever the quotient -- its `floatx80_modrem` works the
/// quotient out for `fprem`, and `fprem1` hands it a null pointer instead --
/// so under QEMU without hardware virtualisation, which is where SlateOS's
/// boot test runs, `remquol(10, 3)` answered a quotient of 0
/// (`services/ctest-longdouble` 72). Theirs would too.
///
/// `fprem`'s bits are reported there as on hardware, and they are the
/// *truncated* quotient's: the rounded one, or one short of it in magnitude.
/// Short exactly when the two remainders differ, since `x - n*y` names `n`
/// -- so both are computed and compared. The remainder returned is still
/// `fprem1`'s, whose value QEMU gets right, and it is exact at any precision
/// control, as neither instruction rounds.
#[must_use]
pub fn remquol(x: L, y: L) -> (L, i32) {
    let r = L::partial_remainder::<true>(x, y).0;
    let (t, n) = L::partial_remainder::<false>(x, y);
    // Unordered -- a NaN, when C leaves the quotient unspecified -- is not
    // "differs": `n` as `fprem` left it.
    let rounded_up = r
        .compare(t)
        .is_some_and(|o| o != core::cmp::Ordering::Equal);
    let q = i32::from(n.wrapping_add(u8::from(rounded_up)) & 7);
    (
        r,
        if x.is_sign_negative() != y.is_sign_negative() {
            -q
        } else {
            q
        },
    )
}

/// The significand in `[0.5, 1)` and the exponent (musl's `frexpl`).
#[must_use]
pub fn frexpl(x: L) -> (L, i32) {
    let ee = i32::from(x.biased_exponent());
    if ee == 0 {
        if x.is_zero() {
            return (x, 0);
        }
        let (r, e) = frexpl(x * L::from_bits(0x3FFF + 120, 1 << 63));
        return (r, e.wrapping_sub(120));
    }
    if ee == 0x7FFF {
        return (x, 0);
    }
    let r = L::from_bits((x.sign_exp & 0x8000) | 0x3FFE, x.significand);
    (r, ee.wrapping_sub(0x3FFE))
}

/// `x * 2^n` as glibc's `s_ldexp_template.c` has it, which `ldexpl`,
/// `scalbnl` and `scalblnl` all are: a zero or non-finite `x` is its own
/// answer, and `ERANGE` when the result overflows or underflows to zero
/// ([`ranged`]).
fn scaled(x: L, n: i32) -> L {
    if !x.is_finite() || x.is_zero() {
        return x + x;
    }
    ranged(
        (x, n),
        |(x, n)| scalbnl_raw(x, n),
        |r| overflow_or_underflow(true, r),
    )
}

/// `x * 2^n`, rounded once (musl's `scalbnl`: scale in at most three exact
/// steps, so the one rounding is the last).
fn scalbnl_raw(x: L, n: i32) -> L {
    let mut x = x;
    let mut n = n;
    let big = L::from_bits(0x3FFF + 16383, 1 << 63);
    let small = L::from_bits(0x3FFF - 16382 + 113, 1 << 63); // 0x1p-16382 * 0x1p113
    if n > 16383 {
        x = x * big;
        n = n.wrapping_sub(16383);
        if n > 16383 {
            x = x * big;
            n = n.wrapping_sub(16383);
            n = n.min(16383);
        }
    } else if n < -16382 {
        x = x * small;
        n = n.wrapping_add(16382 - 113);
        if n < -16382 {
            x = x * small;
            n = n.wrapping_add(16382 - 113);
            n = n.max(-16382);
        }
    }
    let scale = L::from_bits((0x3FFF_i32.wrapping_add(n)) as u16, 1 << 63);
    x * scale
}

/// `x * 2^n`.
#[must_use]
pub fn scalbnl(x: L, n: i32) -> L {
    scaled(x, n)
}

/// [`scalbnl`].
#[must_use]
pub fn ldexpl(x: L, n: i32) -> L {
    scalbnl(x, n)
}

/// [`scalbnl`] with a `long` exponent, clamped: past `+-2^31` every long
/// double has overflowed or underflowed long since.
#[must_use]
pub fn scalblnl(x: L, n: i64) -> L {
    scalbnl(
        x,
        i32::try_from(n).unwrap_or(if n < 0 { i32::MIN } else { i32::MAX }),
    )
}

/// `FP_ILOGB0`, glibc's on x86-64.
const FP_ILOGB0: i32 = i32::MIN;
/// `FP_ILOGBNAN`, glibc's on x86-64.
const FP_ILOGBNAN: i32 = i32::MIN;

/// The unbiased exponent (musl's `ilogbl`); `EDOM` for zero, NaN and
/// infinity, as glibc's `w_ilogb_template.c` sets it.
#[must_use]
pub fn ilogbl(x: L) -> i32 {
    let r = ilogbl_raw(x);
    if r == FP_ILOGB0 || r == FP_ILOGBNAN || r == i32::MAX {
        set(errno::EDOM);
    }
    r
}

fn ilogbl_raw(x: L) -> i32 {
    let m = x.significand;
    let e = i32::from(x.biased_exponent());
    if e == 0 {
        if m == 0 {
            raise_invalid();
            return FP_ILOGB0;
        }
        // Subnormal: count the leading zeros below the integer bit.
        #[allow(clippy::cast_possible_wrap)]
        let lz = m.leading_zeros() as i32;
        return -0x3FFF + 1 - lz;
    }
    if m >> 63 == 0 {
        // An encoding the x87 refuses -- an unnormal, a pseudo-infinity or a
        // pseudo-NaN: glibc's `e_ilogbl.S` hands it to `fxtract`, which
        // raises invalid and makes a NaN of it, whose exponent `fistp`
        // stores as FP_ILOGBNAN.
        raise_invalid();
        return FP_ILOGBNAN;
    }
    if e == 0x7FFF {
        raise_invalid();
        return if m << 1 != 0 { FP_ILOGBNAN } else { i32::MAX };
    }
    e - 0x3FFF
}

/// The exponent as a long double: glibc's x86 `logbl`, which is the unit's
/// `fxtract` -- `-inf` and divide-by-zero for a zero, `+inf` for an
/// infinity, a NaN quieted, and for an encoding the unit refuses (an
/// unnormal, a pseudo-infinity or a pseudo-NaN) its NaN, with invalid.
/// musl's bit arithmetic, which this was, answered those as numbers.
#[must_use]
pub fn logbl(x: L) -> L {
    x.fxtract().1
}

/// The fractional part, and the integral part through `iptr` (musl's
/// `modfl`).
#[must_use]
pub fn modfl(x: L) -> (L, L) {
    let e = i32::from(x.biased_exponent()) - 0x3FFF;
    let neg = x.is_sign_negative();
    let signed_zero = ZERO.copysign(x);
    if e >= 64 - 1 {
        if x.is_nan() {
            return (x, x);
        }
        return (signed_zero, x);
    }
    if e < 0 {
        return (x, signed_zero);
    }
    let absx = if neg { -x } else { x };
    let mut y = absx + TOINT - TOINT - absx;
    if y.is_zero() {
        return (signed_zero, x);
    }
    if y > ZERO {
        y = y - ONE;
    }
    if neg {
        y = -y;
    }
    (-y, x + y)
}

/// `x - y` if positive, else +0; `ERANGE` when finite arguments overflow
/// (glibc's `s_fdim_template.c`, [`ranged`]).
#[must_use]
pub fn fdiml(x: L, y: L) -> L {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    if x <= y {
        return ZERO;
    }
    ranged(
        (x, y),
        |(x, y)| x - y,
        |r| overflow_only(!x.is_infinite() && !y.is_infinite(), r),
    )
}

/// A signalling NaN: the quiet bit (62) clear.
fn signaling(x: L) -> bool {
    x.is_nan() && x.significand & (1 << 62) == 0
}

/// The larger, a quiet NaN losing to a number -- glibc's `s_fmaxl.c`
/// exactly, which does not order the zeros: for equal arguments it answers
/// `y`, so `fmaxl(+0, -0)` is `-0` (C leaves the choice open).
#[must_use]
pub fn fmaxl(x: L, y: L) -> L {
    if x <= y {
        return y;
    }
    if x > y {
        return x;
    }
    if signaling(x) || signaling(y) {
        return x + y;
    }
    if y.is_nan() { x } else { y }
}

/// The smaller, a quiet NaN losing to a number -- glibc's `s_fminl.c`: for
/// equal arguments it answers `x`.
#[must_use]
pub fn fminl(x: L, y: L) -> L {
    if x <= y {
        return x;
    }
    if x > y {
        return y;
    }
    if signaling(x) || signaling(y) {
        return x + y;
    }
    if y.is_nan() { x } else { y }
}

/// glibc's `s_nextafter.c` (and `nexttoward`): `ERANGE` when the step from a
/// finite nonzero `x` overflows or lands on a subnormal or zero.
fn next_errno(x: L, r: L) {
    let e = r.biased_exponent();
    if !x.is_zero() && x.is_finite() && (e == 0x7FFF || e == 0) && !r.is_nan() {
        set(errno::ERANGE);
    }
}

/// The long double after `x` in `y`'s direction (musl's `nextafterl`).
#[must_use]
pub fn nextafterl(x: L, y: L) -> L {
    if x.is_nan() || y.is_nan() {
        return x + y;
    }
    if x == y {
        return y;
    }
    let r = next_raw(x, y > x);
    next_errno(x, r);
    r
}

/// One step up (`up`) or down, in the 80-bit format's ordering, with the
/// explicit integer bit kept right across the subnormal/normal boundary.
fn next_raw(x: L, up: bool) -> L {
    if x.is_zero() {
        return L::from_bits(if up { 0 } else { 0x8000 }, 1);
    }
    let away = up != x.is_sign_negative();
    let mut se = x.sign_exp;
    let mut m = x.significand;
    if away {
        m = m.wrapping_add(1);
        if m == 0 {
            // Carried out of the significand: the next binade.
            m = 1 << 63;
            se = se.wrapping_add(1);
        } else if L::from_bits(se, m).biased_exponent() == 0 && m == 1 << 63 {
            // The largest subnormal stepped into the smallest normal.
            se = se.wrapping_add(1);
        }
    } else {
        if m == 1 << 63 && se & 0x7FFF > 1 {
            // Down out of a binade: its predecessor has all bits set.
            m = u64::MAX;
            se = se.wrapping_sub(1);
        } else if m == 1 << 63 && se & 0x7FFF == 1 {
            // The smallest normal down to the largest subnormal.
            m = (1 << 63) - 1;
            se = se.wrapping_sub(1);
        } else {
            m = m.wrapping_sub(1);
        }
    }
    let r = L::from_bits(se, m);
    // Raise what the step raised: overflow to infinity, or inexact with
    // underflow into the subnormals.
    if r.biased_exponent() == 0x7FFF {
        force_eval(r * r);
    } else if r.biased_exponent() == 0 {
        force_eval(r * r);
    }
    r
}

/// [`nextafterl`] toward a long double (`nexttowardl`).
#[must_use]
pub fn nexttowardl(x: L, y: L) -> L {
    nextafterl(x, y)
}

/// The double after `x` toward the long double `y`.
#[must_use]
pub fn nexttoward(x: f64, y: L) -> f64 {
    let lx = L::from_f64(x);
    if x.is_nan() || y.is_nan() {
        return (lx + y).to_f64();
    }
    if lx == y {
        return y.to_f64();
    }
    let r = libm::nextafter(
        x,
        if y > lx {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        },
    );
    let e = (r.to_bits() >> 52) & 0x7FF;
    if x != 0.0 && x.is_finite() && (e == 0x7FF || e == 0) && !r.is_nan() {
        set(errno::ERANGE);
    }
    r
}

/// The float after `x` toward the long double `y`.
#[must_use]
pub fn nexttowardf(x: f32, y: L) -> f32 {
    let lx = L::from_f64(f64::from(x));
    if x.is_nan() || y.is_nan() {
        return (lx + y).to_f64() as f32;
    }
    if lx == y {
        #[allow(clippy::cast_possible_truncation)]
        return y.to_f64() as f32;
    }
    let r = libm::nextafterf(
        x,
        if y > lx {
            f32::INFINITY
        } else {
            f32::NEG_INFINITY
        },
    );
    let e = (r.to_bits() >> 23) & 0xFF;
    if x != 0.0 && x.is_finite() && (e == 0xFF || e == 0) && !r.is_nan() {
        set(errno::ERANGE);
    }
    r
}

// ===========================================================================
// Logarithms and inverse trigonometry: one x87 instruction each
// ===========================================================================

/// glibc's `w_log_template.c` (and `log2`, `log10`): `ERANGE` at zero (a
/// pole), `EDOM` below.
fn log_errno(x: L) {
    if x.is_zero() {
        set(errno::ERANGE);
    } else if x < ZERO {
        set(errno::EDOM);
    }
}

/// The natural logarithm: `ln 2 * log2 x` (`fyl2x`).
#[must_use]
pub fn logl(x: L) -> L {
    log_errno(x);
    logl_raw(x)
}

/// [`logl`] without `errno`, for the functions built on it.
fn logl_raw(x: L) -> L {
    L::fyl2x(L::LN_2, x)
}

/// The base-2 logarithm.
#[must_use]
pub fn log2l(x: L) -> L {
    log_errno(x);
    L::fyl2x(ONE, x)
}

/// The base-10 logarithm.
#[must_use]
pub fn log10l(x: L) -> L {
    log_errno(x);
    L::fyl2x(L::LOG10_2, x)
}

/// `ln(1 + x)`: `fyl2xp1` where it is accurate, `|x| <= 0.2890625` (musl's
/// threshold, just under `1 - sqrt(2)/2`), else `ln(1 + x)` directly.
/// `ERANGE` at -1, `EDOM` below (glibc's `w_log1p_template.c`).
#[must_use]
pub fn log1pl(x: L) -> L {
    if x == -ONE {
        set(errno::ERANGE);
    } else if x < -ONE {
        set(errno::EDOM);
    }
    log1pl_raw(x)
}

/// [`log1pl`] without `errno`, for the functions built on it.
///
/// Past 2^65, `ln(x)` itself: `1 + x` rounds to `x` there to nearest, and
/// `ln(1 + x) - ln(x) < 2^-65` is far below half an ulp of a logarithm over
/// 45 -- while rounding upward, `1 + LDBL_MAX` is an infinity, which made
/// `log1pl(LDBL_MAX)` one.
fn log1pl_raw(x: L) -> L {
    let key = (u32::from(x.sign_exp & 0x7FFF) << 16) | ((x.significand >> 48) as u32);
    if key <= 0x3FFD_9400 {
        L::fyl2xp1(L::LN_2, x)
    } else if !x.is_sign_negative() && x.biased_exponent() >= 0x3FFF + 65 {
        L::fyl2x(L::LN_2, x)
    } else {
        L::fyl2x(L::LN_2, ONE + x)
    }
}

/// The arctangent (`fpatan` of `x / 1`).
#[must_use]
pub fn atanl(x: L) -> L {
    L::fpatan(x, ONE)
}

/// The angle of (`x`, `y`) -- `y` first (`fpatan`). `ERANGE` when a nonzero
/// `y` and finite `x` underflow to zero (glibc's `w_atan2_template.c`).
#[must_use]
pub fn atan2l(y: L, x: L) -> L {
    ranged(
        (y, x),
        |(y, x)| L::fpatan(y, x),
        |z| underflow_only(z.is_zero() && !y.is_zero() && x.is_finite()),
    )
}

/// `EDOM` for `|x| > 1` (glibc's `w_asin_template.c`, `w_acos_template.c`).
fn unit_domain(x: L) {
    if x.abs() > ONE {
        set(errno::EDOM);
    }
}

/// `sqrt((1 - x)(1 + x))`, musl's cancellation-free `sqrt(1 - x^2)`.
fn cos_of_asin(x: L) -> L {
    ((ONE - x) * (ONE + x)).sqrt()
}

/// The arcsine: `atan2(x, sqrt(1 - x^2))`.
#[must_use]
pub fn asinl(x: L) -> L {
    unit_domain(x);
    L::fpatan(x, cos_of_asin(x))
}

/// The arccosine: `atan2(sqrt(1 - x^2), x)`.
#[must_use]
pub fn acosl(x: L) -> L {
    unit_domain(x);
    L::fpatan(cos_of_asin(x).abs(), x)
}

// ===========================================================================
// Exponentials
// ===========================================================================

/// glibc's `w_exp_template.c` (and `exp2`, `exp10`): `ERANGE` when a finite
/// argument overflows or underflows to zero ([`ranged`]).
fn exp_like(x: L, f: fn(L) -> L) -> L {
    ranged(x, f, |r| overflow_or_underflow(x.is_finite(), r))
}

/// `2^x` (musl's `exp2l.s`): `2^rint(x)` built exactly in the exponent
/// field, times `2^(x - rint x)` from `f2xm1`.
fn exp2l_raw(x: L) -> L {
    let e = x.biased_exponent();
    if e >= 0x3FFF + 15 {
        // |x| >= 32768, an infinity or a NaN: `fscale` overflows, underflows
        // or propagates exactly as 2^x must.
        return ONE.fscale(x);
    }
    if e >= 0x3FFF + 13 {
        // 8192 <= |x| < 32768.
        if x.is_sign_negative() && x <= ld(-16382.0) {
            // Deep in the subnormals: raise underflow unless x is an integer,
            // whose result is exact (musl raises it with a float division).
            let t = ld(9_223_372_036_854_775_808.0); // 2^63
            if x - t + t != x {
                let tiny = core::hint::black_box(f32::from_bits(1));
                #[allow(clippy::cast_possible_truncation)]
                let _ = core::hint::black_box(tiny / (x.to_f64() as f32));
            }
        }
        let r = x.round_int();
        let f = (x - r).f2xm1() + ONE;
        return f.fscale(r);
    }
    if e < 0x3FFF - 64 {
        // |x| < 2^-64: 1, exactly as fscale(1, trunc x) gives it.
        return ONE.fscale(x);
    }
    // |x| < 8192: rint(x) fits the exponent field directly.
    let n = x.to_i64_rint();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let two_n = L::from_bits((0x3FFF + n) as u16, 1 << 63);
    let f = (x - L::from_i64(n)).f2xm1() + ONE;
    f * two_n
}

/// `2^x`.
#[must_use]
pub fn exp2l(x: L) -> L {
    exp_like(x, exp2l_raw)
}

/// `e^x` (musl's `expl.s`): `2^hi * 2^lo`, where `hi + lo` is `x log2 e` to
/// about 128 bits, by Dekker and Veltkamp's exact products.
///
/// Departs from musl for `|x| >= 2^14`: musl answers `2^x` there, which is
/// wrong -- a subnormal instead of zero -- for `x` in `(-16445, -16384]`.
/// Every such `x` overflows or underflows `e^x` to infinity or zero, which
/// `2^(2x)` does exactly the same (propagating NaN and infinities as it
/// should), so that is the answer here.
fn expl_raw(x: L) -> L {
    let e = x.biased_exponent();
    if e < 0x3FFF - 32 {
        // |x| < 2^-32: 1 + x, rounded.
        return ONE + x;
    }
    if e >= 0x3FFF + 14 {
        return ONE.fscale(x + x);
    }
    let hi = L::LOG2_E * x;
    let two_hi = exp2l_raw(hi);
    if two_hi.biased_exponent() == 0x7FFF {
        return two_hi;
    }
    // x = xh + xl and log2e = yh + yl + log2e_lo, split so that every
    // product below is exact in 64 bits.
    let c = ld(4_294_967_297.0); // 2^32 + 1
    let cx = c * x;
    let xh = (x - cx) + cx;
    let xl = x - xh;
    let yh = ld(f64::from_bits(0x3FF7_1547_6520_0000));
    let yl = ld(f64::from_bits(0x3DE7_05FC_2F00_0000));
    let log2e_lo = L::from_bits(0xBFBE, 0x82F0_025F_2DC5_82EE);
    let lo = (xh * yh - hi) + xl * yh + (xh * yl + xl * yl) + log2e_lo * x;
    two_hi * lo.f2xm1() + two_hi
}

/// `e^x`.
#[must_use]
pub fn expl(x: L) -> L {
    exp_like(x, expl_raw)
}

/// `e^x - 1` (musl's `expm1l`, in `exp2l.s`): `f2xm1(x log2 e)` for
/// `|x log2 e| <= 1`, and -1 once `x log2 e <= -65`.
///
/// Departs from musl in between: musl takes `2^(x log2 e) - 1` with
/// `x log2 e` already rounded, which for large `x` loses as many bits as
/// the exponent has; `expl(x) - 1` does not. `ERANGE` on overflow (glibc's
/// `w_expm1_template.c`).
#[must_use]
pub fn expm1l(x: L) -> L {
    ranged(x, expm1l_raw, |r| overflow_only(x.is_finite(), r))
}

/// [`expm1l`] without `errno`, for the functions built on it.
fn expm1l_raw(x: L) -> L {
    let y = L::LOG2_E * x;
    if y <= ld(-65.0) {
        -ONE
    } else if y.abs() > ONE {
        expl_raw(x) - ONE
    } else {
        // |y| <= 1, or a NaN, which f2xm1 propagates.
        y.f2xm1()
    }
}

// ===========================================================================
// Hyperbolic functions and their inverses, cube root, hypotenuse
// ===========================================================================

/// The top 32 bits of the significand, musl's `u.i.m >> 32`.
fn hi32(x: L) -> u32 {
    (x.significand >> 32) as u32
}

/// The hyperbolic sine (musl's ld80 `sinhl`); `ERANGE` when a finite
/// argument overflows (glibc's `w_sinh_template.c`, [`ranged`]).
#[must_use]
pub fn sinhl(x: L) -> L {
    ranged(x, sinhl_raw, |r| overflow_only(x.is_finite(), r))
}

fn sinhl_raw(x: L) -> L {
    let ex = x.biased_exponent();
    let h = if x.is_sign_negative() {
        ld(-0.5)
    } else {
        ld(0.5)
    };
    let absx = x.abs();
    // |x| < log(LDBL_MAX)
    if ex < 0x3FFF + 13 || (ex == 0x3FFF + 13 && hi32(absx) < 0xB172_17F7) {
        let t = expm1l_raw(absx);
        if ex < 0x3FFF {
            if ex < 0x3FFF - 32 {
                return x;
            }
            h * (ld(2.0) * t - t * t / (ONE + t))
        } else {
            h * (t + t / (t + ONE))
        }
    } else {
        // |x| > log(LDBL_MAX), or NaN
        let t = expl_raw(ld(0.5) * absx);
        h * t * t
    }
}

/// The hyperbolic cosine (musl's ld80 `coshl`); `ERANGE` as [`sinhl`].
#[must_use]
pub fn coshl(x: L) -> L {
    ranged(x, coshl_raw, |r| overflow_only(x.is_finite(), r))
}

fn coshl_raw(x: L) -> L {
    let ex = x.biased_exponent();
    let a = x.abs();
    let w = hi32(a);
    if ex < 0x3FFF - 1 || (ex == 0x3FFF - 1 && w < 0xB172_17F7) {
        // |x| < log 2
        if ex < 0x3FFF - 32 {
            force_eval(a + L::from_bits(0x3FFF + 120, 1 << 63));
            return ONE;
        }
        let t = expm1l_raw(a);
        ONE + t * t / (ld(2.0) * (ONE + t))
    } else if ex < 0x3FFF + 13 || (ex == 0x3FFF + 13 && w < 0xB172_17F7) {
        // |x| < log(LDBL_MAX)
        let t = expl_raw(a);
        ld(0.5) * (t + ONE / t)
    } else {
        // |x| > log(LDBL_MAX), or NaN
        let t = expl_raw(ld(0.5) * a);
        ld(0.5) * t * t
    }
}

/// The hyperbolic tangent (musl's ld80 `tanhl`).
#[must_use]
pub fn tanhl(x: L) -> L {
    let ex = x.biased_exponent();
    let a = x.abs();
    let w = hi32(a);
    let t = if ex > 0x3FFE || (ex == 0x3FFE && w > 0x8C9F_53D5) {
        // |x| > log(3)/2 ~= 0.5493, or NaN
        if ex >= 0x3FFF + 5 {
            // |x| >= 32: 1, raising inexact.
            ONE + ZERO / (a + L::from_bits(0x3FFF - 120, 1 << 63))
        } else {
            let t = expm1l_raw(ld(2.0) * a);
            ONE - ld(2.0) / (t + ld(2.0))
        }
    } else if ex > 0x3FFD || (ex == 0x3FFD && w > 0x82C5_77D4) {
        // |x| > log(5/3)/2 ~= 0.2554
        let t = expm1l_raw(ld(2.0) * a);
        t / (t + ld(2.0))
    } else {
        // |x| is small
        let t = expm1l_raw(ld(-2.0) * a);
        -t / (t + ld(2.0))
    };
    if x.is_sign_negative() { -t } else { t }
}

/// The inverse hyperbolic sine (musl's ld80 `asinhl`).
#[must_use]
pub fn asinhl(x: L) -> L {
    let e = x.biased_exponent();
    let a = x.abs();
    let r = if e >= 0x3FFF + 32 {
        // |x| >= 2^32, an infinity or a NaN
        logl_raw(a) + L::LN_2
    } else if e > 0x3FFF {
        // |x| >= 2
        logl_raw(ld(2.0) * a + ONE / ((a * a + ONE).sqrt() + a))
    } else if e >= 0x3FFF - 32 {
        // |x| >= 2^-32
        log1pl_raw(a + a * a / ((a * a + ONE).sqrt() + ONE))
    } else {
        // |x| < 2^-32: raise inexact if x != 0
        force_eval(a + L::from_bits(0x3FFF + 120, 1 << 63));
        a
    };
    if x.is_sign_negative() { -r } else { r }
}

/// The inverse hyperbolic cosine (musl's ld80 `acoshl`); `EDOM` below 1
/// (glibc's `w_acosh_template.c`).
#[must_use]
pub fn acoshl(x: L) -> L {
    if x < ONE {
        set(errno::EDOM);
    }
    if x == ONE {
        // +0 in every direction (Annex F); musl's `x - 1` is -0 downward.
        return ZERO;
    }
    let se = x.sign_exp;
    if se < 0x3FFF + 1 {
        // 0 <= x < 2, invalid if x < 1
        let xm1 = x - ONE;
        return log1pl_raw(xm1 + (xm1 * xm1 + ld(2.0) * xm1).sqrt());
    }
    if se < 0x3FFF + 32 {
        // 2 <= x < 2^32
        return logl_raw(ld(2.0) * x - ONE / (x + (x * x - ONE).sqrt()));
    }
    if se & 0x8000 != 0 {
        // x < 0 or x = -0: invalid
        return (x - x) / (x - x);
    }
    // 2^32 <= x, or NaN
    logl_raw(x) + L::LN_2
}

/// The inverse hyperbolic tangent: musl's ld80 `atanhl`, with the
/// thresholds it meant. musl compares the exponent with `0x3ff - 1`, the
/// *double* bias, so its accurate branch for `|x| < 0.5` is never taken for
/// a long double; here the comparison is with `0x3fff - 1`. `ERANGE` at +-1,
/// `EDOM` beyond (glibc's `w_atanh_template.c`).
#[must_use]
pub fn atanhl(x: L) -> L {
    let a = x.abs();
    if a >= ONE {
        set(if a == ONE { errno::ERANGE } else { errno::EDOM });
    }
    if a == ONE {
        // The pole: an infinity of x's sign, raising divide-by-zero, in
        // every direction -- musl's `a / (1 - a)` divides by -0 downward.
        return x / ZERO;
    }
    let e = x.biased_exponent();
    let r = if e < 0x3FFF - 1 {
        if e < 0x3FFF - 32 {
            // |x| < 2^-32: atanh(x) = x; underflow for a subnormal.
            if e == 0 {
                #[allow(clippy::cast_possible_truncation)]
                let _ = core::hint::black_box(a.to_f64() as f32);
            }
            a
        } else {
            // |x| < 0.5
            ld(0.5) * log1pl_raw(ld(2.0) * a + ld(2.0) * a * a / (ONE - a))
        }
    } else {
        // Avoid overflow.
        ld(0.5) * log1pl_raw(ld(2.0) * (a / (ONE - a)))
    };
    if x.is_sign_negative() { -r } else { r }
}

/// The cube root (musl's ld80 `cbrtl`): a float estimate, two double Newton
/// steps, then one in long double.
#[must_use]
pub fn cbrtl(x: L) -> L {
    const B1: u32 = 709_958_130;
    let mut e = i32::from(x.biased_exponent());
    let sign = x.sign_exp & 0x8000;
    if e == 0x7FFF {
        return x + x;
    }
    let mut u = x;
    if e == 0 {
        // Adjust subnormal numbers.
        u = u * L::from_bits(0x3FFF + 120, 1 << 63);
        e = i32::from(u.biased_exponent());
        if e == 0 {
            // +-0
            return x;
        }
        e -= 120;
    }
    e -= 0x3FFF;
    let mut xr = L::from_bits(0x3FFF, u.significand);
    match e.rem_euclid(3) {
        1 => {
            xr = xr * ld(2.0);
            e -= 1;
        }
        2 => {
            xr = xr * ld(4.0);
            e -= 2;
        }
        _ => {}
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let v = L::from_bits(sign | ((0x3FFF + e / 3) as u16), 1 << 63);
    // ~5-bit estimate, in float bits
    #[allow(clippy::cast_possible_truncation)]
    let fx = xr.to_f64() as f32;
    let ft = f32::from_bits((fx.to_bits() & 0x7FFF_FFFF) / 3 + B1);
    // ~16-bit, then ~47-bit estimates, in double
    let dx = xr.to_f64();
    let mut dt = f64::from(ft);
    let mut dr = dt * dt * dt;
    dt = dt * (dx + dx + dr) / (dx + dr + dr);
    dr = dt * dt * dt;
    dt = dt * (dx + dx + dr) / (dx + dr + dr);
    // Round dt away from zero to 32 bits, so that t*t is exact.
    let two32 = ld(4_294_967_296.0);
    let t = ld(dt) + (two32 + L::from_bits(0x3FFF - 31, 1 << 63)) - two32;
    // One Newton step in long double: error < 0.667 ulp.
    let s = t * t;
    let r = xr / s;
    let w = t + t;
    let r = (r - t) / (w + r);
    let t = t + t * r;
    t * v
}

/// `x*x` as `hi + lo` exactly, by Dekker's split (musl's `hypotl`'s `sq`).
fn sq(x: L) -> (L, L) {
    let xc = x * ld(4_294_967_297.0); // 2^32 + 1
    let xh = x - xc + xc;
    let xl = x - xh;
    let hi = x * x;
    let lo = xh * xh - hi + ld(2.0) * xh * xl + xl * xl;
    (hi, lo)
}

/// `sqrt(x^2 + y^2)` without the intermediate overflow (musl's ld80
/// `hypotl`); `ERANGE` when finite arguments overflow.
#[must_use]
pub fn hypotl(x: L, y: L) -> L {
    ranged(
        (x, y),
        |(x, y)| hypotl_raw(x, y),
        |r| overflow_only(x.is_finite() && y.is_finite(), r),
    )
}

fn hypotl_raw(x: L, y: L) -> L {
    let (mut a, mut b) = (x.abs(), y.abs());
    if a.biased_exponent() < b.biased_exponent() {
        core::mem::swap(&mut a, &mut b);
    }
    let (ex, ey) = (
        i32::from(a.biased_exponent()),
        i32::from(b.biased_exponent()),
    );
    if ex == 0x7FFF && b.is_infinite() {
        b
    } else if ex == 0x7FFF || b.is_zero() {
        a
    } else if ex - ey > 64 {
        a + b
    } else {
        let mut z = ONE;
        if ex > 0x3FFF + 8000 {
            z = L::from_bits(0x3FFF + 10000, 1 << 63);
            a = a * L::from_bits(0x3FFF - 10000, 1 << 63);
            b = b * L::from_bits(0x3FFF - 10000, 1 << 63);
        } else if ey < 0x3FFF - 8000 {
            z = L::from_bits(0x3FFF - 10000, 1 << 63);
            a = a * L::from_bits(0x3FFF + 10000, 1 << 63);
            b = b * L::from_bits(0x3FFF + 10000, 1 << 63);
        }
        let (hx, lx) = sq(a);
        let (hy, ly) = sq(b);
        z * (ly + lx + hy + hx).sqrt()
    }
}

// ===========================================================================
// Powers: musl's ld80 `powl` (Cephes) -- its special cases and small
// integer powers; the general case is "Accurate powers" below -- and `exp10l`
// ===========================================================================

const POW_MAXLOGL: L = L::from_bits(0x400C, 0xB17217F7D1CF79AC); // 1.1356523406294143949492E4
const POW_MINLOGL: L = L::from_bits(0xC00C, 0xB21DFE7F09E2BAAA); // -1.13994985314888605586758E4
const POW_LOGE2L: L = L::from_bits(0x3FFE, 0xB17217F7D1CF79AC); // 6.9314718055994530941723E-1
const POW_SQRTH: L = L::from_bits(0x3FFE, 0xB504F333F9DE6433); // 7.0710678118654752e-1
const POW_POWIL_K: L = L::from_bits(0x4000, 0xBA827999FCEF3161); // 2.9142135623730950

// ===========================================================================
// Accurate powers: y * log2(x) to about 80 bits, then 2^that
// ===========================================================================
//
// musl's Cephes `powl` computes log2(x) to about 64 bits, and `y` multiplies
// its error: for |y log2 x| in the thousands its answers are off by tens to
// hundreds of ulps, where glibc's are within one or two. So the general case
// here is computed afresh, with the standard technique for it -- a
// table-driven logarithm carried as a pair of long doubles (`hi + lo`), an
// exact product with `y` by Dekker's method, and one exponential at the end
// -- keeping musl's special cases, which are exact, and its repeated
// squaring for the small integer powers glibc also squares for.

/// `1/c_k` to 9 significant bits, `c_k = 1 + (k + 0.5)/128`: small enough
/// that a 32-bit half of a significand times it is exact in 64 bits.
const LOG2_INV: [L; 128] = [
    L::from_bits(0x3FFE, 0xFF00000000000000), // 255/256
    L::from_bits(0x3FFE, 0xFD00000000000000), // 253/256
    L::from_bits(0x3FFE, 0xFB00000000000000), // 251/256
    L::from_bits(0x3FFE, 0xF900000000000000), // 249/256
    L::from_bits(0x3FFE, 0xF780000000000000), // 495/512
    L::from_bits(0x3FFE, 0xF580000000000000), // 491/512
    L::from_bits(0x3FFE, 0xF380000000000000), // 487/512
    L::from_bits(0x3FFE, 0xF200000000000000), // 121/128
    L::from_bits(0x3FFE, 0xF000000000000000), // 15/16
    L::from_bits(0x3FFE, 0xEE80000000000000), // 477/512
    L::from_bits(0x3FFE, 0xEC80000000000000), // 473/512
    L::from_bits(0x3FFE, 0xEB00000000000000), // 235/256
    L::from_bits(0x3FFE, 0xE900000000000000), // 233/256
    L::from_bits(0x3FFE, 0xE780000000000000), // 463/512
    L::from_bits(0x3FFE, 0xE600000000000000), // 115/128
    L::from_bits(0x3FFE, 0xE480000000000000), // 457/512
    L::from_bits(0x3FFE, 0xE300000000000000), // 227/256
    L::from_bits(0x3FFE, 0xE100000000000000), // 225/256
    L::from_bits(0x3FFE, 0xDF80000000000000), // 447/512
    L::from_bits(0x3FFE, 0xDE00000000000000), // 111/128
    L::from_bits(0x3FFE, 0xDC80000000000000), // 441/512
    L::from_bits(0x3FFE, 0xDB00000000000000), // 219/256
    L::from_bits(0x3FFE, 0xD980000000000000), // 435/512
    L::from_bits(0x3FFE, 0xD880000000000000), // 433/512
    L::from_bits(0x3FFE, 0xD700000000000000), // 215/256
    L::from_bits(0x3FFE, 0xD580000000000000), // 427/512
    L::from_bits(0x3FFE, 0xD400000000000000), // 53/64
    L::from_bits(0x3FFE, 0xD280000000000000), // 421/512
    L::from_bits(0x3FFE, 0xD180000000000000), // 419/512
    L::from_bits(0x3FFE, 0xD000000000000000), // 13/16
    L::from_bits(0x3FFE, 0xCE80000000000000), // 413/512
    L::from_bits(0x3FFE, 0xCD80000000000000), // 411/512
    L::from_bits(0x3FFE, 0xCC00000000000000), // 51/64
    L::from_bits(0x3FFE, 0xCB00000000000000), // 203/256
    L::from_bits(0x3FFE, 0xC980000000000000), // 403/512
    L::from_bits(0x3FFE, 0xC880000000000000), // 401/512
    L::from_bits(0x3FFE, 0xC700000000000000), // 199/256
    L::from_bits(0x3FFE, 0xC600000000000000), // 99/128
    L::from_bits(0x3FFE, 0xC500000000000000), // 197/256
    L::from_bits(0x3FFE, 0xC380000000000000), // 391/512
    L::from_bits(0x3FFE, 0xC280000000000000), // 389/512
    L::from_bits(0x3FFE, 0xC180000000000000), // 387/512
    L::from_bits(0x3FFE, 0xC000000000000000), // 3/4
    L::from_bits(0x3FFE, 0xBF00000000000000), // 191/256
    L::from_bits(0x3FFE, 0xBE00000000000000), // 95/128
    L::from_bits(0x3FFE, 0xBD00000000000000), // 189/256
    L::from_bits(0x3FFE, 0xBC00000000000000), // 47/64
    L::from_bits(0x3FFE, 0xBA80000000000000), // 373/512
    L::from_bits(0x3FFE, 0xB980000000000000), // 371/512
    L::from_bits(0x3FFE, 0xB880000000000000), // 369/512
    L::from_bits(0x3FFE, 0xB780000000000000), // 367/512
    L::from_bits(0x3FFE, 0xB680000000000000), // 365/512
    L::from_bits(0x3FFE, 0xB580000000000000), // 363/512
    L::from_bits(0x3FFE, 0xB480000000000000), // 361/512
    L::from_bits(0x3FFE, 0xB380000000000000), // 359/512
    L::from_bits(0x3FFE, 0xB280000000000000), // 357/512
    L::from_bits(0x3FFE, 0xB180000000000000), // 355/512
    L::from_bits(0x3FFE, 0xB080000000000000), // 353/512
    L::from_bits(0x3FFE, 0xAF80000000000000), // 351/512
    L::from_bits(0x3FFE, 0xAF00000000000000), // 175/256
    L::from_bits(0x3FFE, 0xAE00000000000000), // 87/128
    L::from_bits(0x3FFE, 0xAD00000000000000), // 173/256
    L::from_bits(0x3FFE, 0xAC00000000000000), // 43/64
    L::from_bits(0x3FFE, 0xAB00000000000000), // 171/256
    L::from_bits(0x3FFE, 0xAA00000000000000), // 85/128
    L::from_bits(0x3FFE, 0xA980000000000000), // 339/512
    L::from_bits(0x3FFE, 0xA880000000000000), // 337/512
    L::from_bits(0x3FFE, 0xA780000000000000), // 335/512
    L::from_bits(0x3FFE, 0xA700000000000000), // 167/256
    L::from_bits(0x3FFE, 0xA600000000000000), // 83/128
    L::from_bits(0x3FFE, 0xA500000000000000), // 165/256
    L::from_bits(0x3FFE, 0xA480000000000000), // 329/512
    L::from_bits(0x3FFE, 0xA380000000000000), // 327/512
    L::from_bits(0x3FFE, 0xA280000000000000), // 325/512
    L::from_bits(0x3FFE, 0xA200000000000000), // 81/128
    L::from_bits(0x3FFE, 0xA100000000000000), // 161/256
    L::from_bits(0x3FFE, 0xA000000000000000), // 5/8
    L::from_bits(0x3FFE, 0x9F80000000000000), // 319/512
    L::from_bits(0x3FFE, 0x9E80000000000000), // 317/512
    L::from_bits(0x3FFE, 0x9E00000000000000), // 79/128
    L::from_bits(0x3FFE, 0x9D00000000000000), // 157/256
    L::from_bits(0x3FFE, 0x9C80000000000000), // 313/512
    L::from_bits(0x3FFE, 0x9B80000000000000), // 311/512
    L::from_bits(0x3FFE, 0x9B00000000000000), // 155/256
    L::from_bits(0x3FFE, 0x9A00000000000000), // 77/128
    L::from_bits(0x3FFE, 0x9980000000000000), // 307/512
    L::from_bits(0x3FFE, 0x9900000000000000), // 153/256
    L::from_bits(0x3FFE, 0x9800000000000000), // 19/32
    L::from_bits(0x3FFE, 0x9780000000000000), // 303/512
    L::from_bits(0x3FFE, 0x9680000000000000), // 301/512
    L::from_bits(0x3FFE, 0x9600000000000000), // 75/128
    L::from_bits(0x3FFE, 0x9580000000000000), // 299/512
    L::from_bits(0x3FFE, 0x9480000000000000), // 297/512
    L::from_bits(0x3FFE, 0x9400000000000000), // 37/64
    L::from_bits(0x3FFE, 0x9380000000000000), // 295/512
    L::from_bits(0x3FFE, 0x9280000000000000), // 293/512
    L::from_bits(0x3FFE, 0x9200000000000000), // 73/128
    L::from_bits(0x3FFE, 0x9180000000000000), // 291/512
    L::from_bits(0x3FFE, 0x9080000000000000), // 289/512
    L::from_bits(0x3FFE, 0x9000000000000000), // 9/16
    L::from_bits(0x3FFE, 0x8F80000000000000), // 287/512
    L::from_bits(0x3FFE, 0x8F00000000000000), // 143/256
    L::from_bits(0x3FFE, 0x8E00000000000000), // 71/128
    L::from_bits(0x3FFE, 0x8D80000000000000), // 283/512
    L::from_bits(0x3FFE, 0x8D00000000000000), // 141/256
    L::from_bits(0x3FFE, 0x8C80000000000000), // 281/512
    L::from_bits(0x3FFE, 0x8B80000000000000), // 279/512
    L::from_bits(0x3FFE, 0x8B00000000000000), // 139/256
    L::from_bits(0x3FFE, 0x8A80000000000000), // 277/512
    L::from_bits(0x3FFE, 0x8A00000000000000), // 69/128
    L::from_bits(0x3FFE, 0x8980000000000000), // 275/512
    L::from_bits(0x3FFE, 0x8900000000000000), // 137/256
    L::from_bits(0x3FFE, 0x8800000000000000), // 17/32
    L::from_bits(0x3FFE, 0x8780000000000000), // 271/512
    L::from_bits(0x3FFE, 0x8700000000000000), // 135/256
    L::from_bits(0x3FFE, 0x8680000000000000), // 269/512
    L::from_bits(0x3FFE, 0x8600000000000000), // 67/128
    L::from_bits(0x3FFE, 0x8580000000000000), // 267/512
    L::from_bits(0x3FFE, 0x8500000000000000), // 133/256
    L::from_bits(0x3FFE, 0x8480000000000000), // 265/512
    L::from_bits(0x3FFE, 0x8400000000000000), // 33/64
    L::from_bits(0x3FFE, 0x8380000000000000), // 263/512
    L::from_bits(0x3FFE, 0x8300000000000000), // 131/256
    L::from_bits(0x3FFE, 0x8280000000000000), // 261/512
    L::from_bits(0x3FFE, 0x8200000000000000), // 65/128
    L::from_bits(0x3FFE, 0x8180000000000000), // 259/512
    L::from_bits(0x3FFE, 0x8100000000000000), // 129/256
    L::from_bits(0x3FFE, 0x8080000000000000), // 257/512
];

/// `-log2(LOG2_INV[k])` as `hi + lo`, to about 128 bits.
const LOG2_TAB: [(L, L); 128] = [
    (
        L::from_bits(0x3FF7, 0xB906CE03541AF537),
        L::from_bits(0x3FB6, 0xF18F977E5D8A37AB),
    ),
    (
        L::from_bits(0x3FF9, 0x8B510F105052285E),
        L::from_bits(0x3FB8, 0x934138E5EF07A621),
    ),
    (
        L::from_bits(0x3FF9, 0xE91D7E2489CFC4DE),
        L::from_bits(0x3FB8, 0xDAE7BD412854F796),
    ),
    (
        L::from_bits(0x3FFA, 0xA3D5039ABB82795A),
        L::from_bits(0x3FB9, 0xA398C44F7125FF18),
    ),
    (
        L::from_bits(0x3FFA, 0xC789ADC083C88CEA),
        L::from_bits(0x3FB5, 0x83335ACDA8C0D624),
    ),
    (
        L::from_bits(0x3FFA, 0xF77BC92845B4EB0A),
        L::from_bits(0xBFB9, 0x8CDA464E21695797),
    ),
    (
        L::from_bits(0x3FFB, 0x93E925D74126D610),
        L::from_bits(0x3FB6, 0xA1FB6AE82549008E),
    ),
    (
        L::from_bits(0x3FFB, 0xA62B07F3457C4070),
        L::from_bits(0x3FBA, 0xA0F337D555652818),
    ),
    (
        L::from_bits(0x3FFB, 0xBEB024B67DDA633A),
        L::from_bits(0xBFB9, 0x975DC0E7A9636091),
    ),
    (
        L::from_bits(0x3FFB, 0xD13666CCC0C2944D),
        L::from_bits(0xBFBA, 0xBD8229E9360458E1),
    ),
    (
        L::from_bits(0x3FFB, 0xEA180512926A0BB6),
        L::from_bits(0xBFBA, 0xCB44A377DF311765),
    ),
    (
        L::from_bits(0x3FFB, 0xFCE4AEE0E88B274A),
        L::from_bits(0xBFBA, 0xD4D2AE3A2F66E175),
    ),
    (
        L::from_bits(0x3FFC, 0x8B12C98C36D37E21),
        L::from_bits(0xBFB8, 0xFDBC295D1941951D),
    ),
    (
        L::from_bits(0x3FFC, 0x949D61EE0D33432F),
        L::from_bits(0x3FBA, 0xB0CFF1784537234D),
    ),
    (
        L::from_bits(0x3FFC, 0x9E37DB2866F2850B),
        L::from_bits(0x3FBA, 0x8958F27B65188246),
    ),
    (
        L::from_bits(0x3FFC, 0xA7E26A6CDD989AF8),
        L::from_bits(0x3FBB, 0xB0C5CAE1ABDF41FA),
    ),
    (
        L::from_bits(0x3FFC, 0xB19D45FA1BE70855),
        L::from_bits(0x3FBA, 0xFAE008FBB5975806),
    ),
    (
        L::from_bits(0x3FFC, 0xBEB024B67DDA633A),
        L::from_bits(0xBFBA, 0x975DC0E7A9636091),
    ),
    (
        L::from_bits(0x3FFC, 0xC891E0A946366F7A),
        L::from_bits(0x3FBB, 0xD8CDE612CCAE5B8B),
    ),
    (
        L::from_bits(0x3FFC, 0xD284A5AA69414202),
        L::from_bits(0x3FBB, 0xEDBAB3AE483F4625),
    ),
    (
        L::from_bits(0x3FFC, 0xDC88AEDC1D1EE96A),
        L::from_bits(0xBFBB, 0xAD415AE1A7156186),
    ),
    (
        L::from_bits(0x3FFC, 0xE69E389698884993),
        L::from_bits(0xBFB9, 0x8A85FB19BBCB7E04),
    ),
    (
        L::from_bits(0x3FFC, 0xF0C580709891E833),
        L::from_bits(0x3FBB, 0xFDD7C237E3EBEEE3),
    ),
    (
        L::from_bits(0x3FFC, 0xF7945566B9118906),
        L::from_bits(0x3FBA, 0xC4F9E73C770D3FF5),
    ),
    (
        L::from_bits(0x3FFD, 0x80ECDDE7D30EA2ED),
        L::from_bits(0x3FBA, 0x994400CA0A258158),
    ),
    (
        L::from_bits(0x3FFD, 0x8618C576FF9E03B3),
        L::from_bits(0x3FBB, 0xDB092DCE9AA120C9),
    ),
    (
        L::from_bits(0x3FFD, 0x8B4E029B1F8AC392),
        L::from_bits(0xBFBC, 0xAF07FA2A1923A7AF),
    ),
    (
        L::from_bits(0x3FFD, 0x908CB743A39E8598),
        L::from_bits(0xBFBC, 0xFE1E647287B42C01),
    ),
    (
        L::from_bits(0x3FFD, 0x94112DDAF3251F47),
        L::from_bits(0x3FBC, 0xAB0BF7A2FBC705E3),
    ),
    (
        L::from_bits(0x3FFD, 0x995FF71B8773432D),
        L::from_bits(0x3FBA, 0x925E378D67CAEE1E),
    ),
    (
        L::from_bits(0x3FFD, 0x9EB895FCE4EF7663),
        L::from_bits(0x3FBA, 0x8DC8A0E2E98D6C05),
    ),
    (
        L::from_bits(0x3FFD, 0xA24E88AF7BEC3B64),
        L::from_bits(0xBFBB, 0x91EADABDD17A878E),
    ),
    (
        L::from_bits(0x3FFD, 0xA7B7DD96762CC3C7),
        L::from_bits(0x3FBB, 0x9D0B5CA5A8E7BB59),
    ),
    (
        L::from_bits(0x3FFD, 0xAB591735ABC724E5),
        L::from_bits(0xBFB9, 0xCA63AABB43A1ECC8),
    ),
    (
        L::from_bits(0x3FFD, 0xB0D38BE560CC188B),
        L::from_bits(0xBFB8, 0xFE17020952995C89),
    ),
    (
        L::from_bits(0x3FFD, 0xB4805441B686A83A),
        L::from_bits(0x3FBC, 0xF35E53BB822D9003),
    ),
    (
        L::from_bits(0x3FFD, 0xBA0C5675DF8C75B3),
        L::from_bits(0xBFBC, 0xFFFC04CE5D55071E),
    ),
    (
        L::from_bits(0x3FFD, 0xBDC4F81679556990),
        L::from_bits(0xBFBC, 0xEC3A295589B0C986),
    ),
    (
        L::from_bits(0x3FFD, 0xC1826C8608FE9952),
        L::from_bits(0xBFBB, 0xA9EB6954B1B4D6C1),
    ),
    (
        L::from_bits(0x3FFD, 0xC727C1FD0A2F6D7E),
        L::from_bits(0x3FBC, 0x83C7A30EA2E66A50),
    ),
    (
        L::from_bits(0x3FFD, 0xCAF17CDA8F827179),
        L::from_bits(0x3FBB, 0xCD9B08E72B3DD45D),
    ),
    (
        L::from_bits(0x3FFD, 0xCEC0375E4C90DC61),
        L::from_bits(0xBFBB, 0x8E904051ADACB2CB),
    ),
    (
        L::from_bits(0x3FFD, 0xD47FCB8C0852F0C1),
        L::from_bits(0xBFBC, 0x802C48281A2EB745),
    ),
    (
        L::from_bits(0x3FFD, 0xD85B3FA7A3407FA8),
        L::from_bits(0x3FBC, 0xF623E38A2C18A2B8),
    ),
    (
        L::from_bits(0x3FFD, 0xDC3BE2BD8D837F7F),
        L::from_bits(0x3FBA, 0x99CF2B3BF6226E80),
    ),
    (
        L::from_bits(0x3FFD, 0xE021C2CF17ED9BDC),
        L::from_bits(0xBFBA, 0xAEA39C22788B1BAA),
    ),
    (
        L::from_bits(0x3FFD, 0xE40CEE16A2FF21C5),
        L::from_bits(0xBFBC, 0xA2753B99B0DC0390),
    ),
    (
        L::from_bits(0x3FFD, 0xE9F7BBB6A1FF9F87),
        L::from_bits(0x3FBC, 0xA64380531A2BE7A5),
    ),
    (
        L::from_bits(0x3FFD, 0xEDF062C61E7F8D2B),
        L::from_bits(0x3FBC, 0xA5D3F7459745B734),
    ),
    (
        L::from_bits(0x3FFD, 0xF1EE88AB283EEEC5),
        L::from_bits(0x3FBA, 0xAA1AED816D993511),
    ),
    (
        L::from_bits(0x3FFD, 0xF5F23CB071E043FF),
        L::from_bits(0x3FBB, 0x9CA4D0C4BEB4D2F4),
    ),
    (
        L::from_bits(0x3FFD, 0xF9FB8E60DB14DC16),
        L::from_bits(0x3FBC, 0xDC1C18D007B792B6),
    ),
    (
        L::from_bits(0x3FFD, 0xFE0A8D88D9B200DD),
        L::from_bits(0xBFBB, 0xAFDEF46589AADA7D),
    ),
    (
        L::from_bits(0x3FFE, 0x810FA51BF65FD771),
        L::from_bits(0x3FBD, 0xB9333AC3D886506E),
    ),
    (
        L::from_bits(0x3FFE, 0x831CEA610CEAFCBC),
        L::from_bits(0x3FB9, 0xE41697A09520FB51),
    ),
    (
        L::from_bits(0x3FFE, 0x852D1EE0BA90C6B0),
        L::from_bits(0x3FBD, 0xD1B0CFE1426E9E48),
    ),
    (
        L::from_bits(0x3FFE, 0x87404B0BDA359FC8),
        L::from_bits(0x3FBC, 0xDDAA8C5DEAA518C6),
    ),
    (
        L::from_bits(0x3FFE, 0x89567777E6C11BFB),
        L::from_bits(0x3FBD, 0xC5AD4754F7248C65),
    ),
    (
        L::from_bits(0x3FFE, 0x8B6FACDFD0360AB8),
        L::from_bits(0xBFBD, 0xADF6A54A7A4CB523),
    ),
    (
        L::from_bits(0x3FFE, 0x8C7D6DB7169E0CDB),
        L::from_bits(0xBFBD, 0xE851773D02C9055C),
    ),
    (
        L::from_bits(0x3FFE, 0x8E9B414B5A92A606),
        L::from_bits(0x3FB9, 0x8D5A8886679D61FB),
    ),
    (
        L::from_bits(0x3FFE, 0x90BC345861BF3D53),
        L::from_bits(0xBFBB, 0x859C64022D8514D2),
    ),
    (
        L::from_bits(0x3FFE, 0x92E050231DF57D70),
        L::from_bits(0xBFBD, 0xA377C7EC513C756E),
    ),
    (
        L::from_bits(0x3FFE, 0x95079E1A0382DC79),
        L::from_bits(0x3FBD, 0xDC6D5539D21470F2),
    ),
    (
        L::from_bits(0x3FFE, 0x973227D6027EBD8A),
        L::from_bits(0x3FBA, 0xEFCA1A184E93808D),
    ),
    (
        L::from_bits(0x3FFE, 0x9848A629936881DD),
        L::from_bits(0xBFBD, 0xD8E7CE35765CBCD5),
    ),
    (
        L::from_bits(0x3FFE, 0x9A781BEB62FD91CC),
        L::from_bits(0x3FBC, 0xA6285239B26AC1A7),
    ),
    (
        L::from_bits(0x3FFE, 0x9CAAE63128F23953),
        L::from_bits(0x3FBA, 0xBDB6949301F2265E),
    ),
    (
        L::from_bits(0x3FFE, 0x9DC58E347D37696D),
        L::from_bits(0x3FBC, 0x99B5B37256454EF0),
    ),
    (
        L::from_bits(0x3FFE, 0x9FFD6A73A78EAF35),
        L::from_bits(0x3FBD, 0x8A4FB0590429BB94),
    ),
    (
        L::from_bits(0x3FFE, 0xA238B5160413106E),
        L::from_bits(0x3FBD, 0x8099576EDAC01C78),
    ),
    (
        L::from_bits(0x3FFE, 0xA357A720D0F9F5AF),
        L::from_bits(0x3FBC, 0xB266B5D5FF8D5B53),
    ),
    (
        L::from_bits(0x3FFE, 0xA5982B6DCCAD1E5D),
        L::from_bits(0xBFBB, 0xA90A8696DF6F864F),
    ),
    (
        L::from_bits(0x3FFE, 0xA7DC392F5ADD49A4),
        L::from_bits(0x3FBC, 0xFF18ADF9A3F751EC),
    ),
    (
        L::from_bits(0x3FFE, 0xA8FF971810A5E181),
        L::from_bits(0x3FBD, 0xFFA76FAFCBA29177),
    ),
    (
        L::from_bits(0x3FFE, 0xAB49080ECDA53209),
        L::from_bits(0xBFBC, 0xF60E61FC9B52CBBE),
    ),
    (
        L::from_bits(0x3FFE, 0xAD961ED0CB91D407),
        L::from_bits(0xBFBC, 0x92BF6FF4DAFDB4CE),
    ),
    (
        L::from_bits(0x3FFE, 0xAEBE0C048AC0F1AD),
        L::from_bits(0xBFBD, 0xAD04FFE637187BBD),
    ),
    (
        L::from_bits(0x3FFE, 0xB110B17D788C15AA),
        L::from_bits(0xBFBA, 0xC3743698D27502A1),
    ),
    (
        L::from_bits(0x3FFE, 0xB23B6CC56CC84C9A),
        L::from_bits(0xBFBD, 0x899B64B03F7230DD),
    ),
    (
        L::from_bits(0x3FFE, 0xB493BC0EC9954244),
        L::from_bits(0xBFBD, 0x9BD1EC6379E6E3B9),
    ),
    (
        L::from_bits(0x3FFE, 0xB5C153293365F274),
        L::from_bits(0xBFBA, 0xE85A908BE583EE71),
    ),
    (
        L::from_bits(0x3FFE, 0xB81F68249DC7B350),
        L::from_bits(0x3FBD, 0xBCB9604191677F3C),
    ),
    (
        L::from_bits(0x3FFE, 0xB94FE935B83E3EB6),
        L::from_bits(0xBFBC, 0xC7386DF8CA1A061E),
    ),
    (
        L::from_bits(0x3FFE, 0xBBB3E094B3D228D4),
        L::from_bits(0xBFBC, 0x9705A795A4E9FC1A),
    ),
    (
        L::from_bits(0x3FFE, 0xBCE75A2AAF55C56C),
        L::from_bits(0x3FBB, 0x9C0B7CFA3FE15CFB),
    ),
    (
        L::from_bits(0x3FFE, 0xBE1BD4913F3FDA44),
        L::from_bits(0xBFBA, 0xC69A675516EB6661),
    ),
    (
        L::from_bits(0x3FFE, 0xC087D28DFB2FEBB9),
        L::from_bits(0xBFBD, 0xA366629E13BCD7C9),
    ),
    (
        L::from_bits(0x3FFE, 0xC1BF598DBEE1B171),
        L::from_bits(0x3FBB, 0xAD598CE7315B074C),
    ),
    (
        L::from_bits(0x3FFE, 0xC43180389D6FE23D),
        L::from_bits(0xBFBD, 0xF909CF347907C5FD),
    ),
    (
        L::from_bits(0x3FFE, 0xC56C23679B4D206E),
        L::from_bits(0x3FBB, 0xB4A9AFDC5FABBE40),
    ),
    (
        L::from_bits(0x3FFE, 0xC6A7D38711E46ED2),
        L::from_bits(0x3FBD, 0xECD69D3B871B4873),
    ),
    (
        L::from_bits(0x3FFE, 0xC92261D140D42D28),
        L::from_bits(0x3FBC, 0x93998E825C207F35),
    ),
    (
        L::from_bits(0x3FFE, 0xCA6143A49626D820),
        L::from_bits(0x3FBC, 0xF709A1FF3E4E5A57),
    ),
    (
        L::from_bits(0x3FFE, 0xCBA139B9BE8F2A6C),
        L::from_bits(0xBFBD, 0xE2149C95E881DC58),
    ),
    (
        L::from_bits(0x3FFE, 0xCE246A2F95A92894),
        L::from_bits(0x3FBD, 0xCAF799AD992C8548),
    ),
    (
        L::from_bits(0x3FFE, 0xCF67A85FA1F89A04),
        L::from_bits(0x3FBD, 0xB76DC462715AA3C2),
    ),
    (
        L::from_bits(0x3FFE, 0xD0AC02705F44421D),
        L::from_bits(0x3FB8, 0xD92F88FA1CD23D3D),
    ),
    (
        L::from_bits(0x3FFE, 0xD338120A6DD9D306),
        L::from_bits(0x3FBD, 0xCEB1F67AEEA294F1),
    ),
    (
        L::from_bits(0x3FFE, 0xD47FCB8C0852F0C1),
        L::from_bits(0xBFBD, 0x802C48281A2EB745),
    ),
    (
        L::from_bits(0x3FFE, 0xD5C8A8DF0B46EB6F),
        L::from_bits(0xBFBD, 0xC022616FDFE972A8),
    ),
    (
        L::from_bits(0x3FFE, 0xD712AC0CF811659E),
        L::from_bits(0xBFBD, 0xE3A50590FDB04FBB),
    ),
    (
        L::from_bits(0x3FFE, 0xD9AA2C3B0EA3CBC1),
        L::from_bits(0x3FBD, 0xB834FE2962D166CA),
    ),
    (
        L::from_bits(0x3FFE, 0xDAF7AD69F0B380E5),
        L::from_bits(0x3FBB, 0x975535BDDD3CD39A),
    ),
    (
        L::from_bits(0x3FFE, 0xDC465CD155A90943),
        L::from_bits(0xBFBD, 0x9150C1E0E5855D6A),
    ),
    (
        L::from_bits(0x3FFE, 0xDD963C96EDF3A402),
        L::from_bits(0xBFBD, 0xD690403E4CCB5F65),
    ),
    (
        L::from_bits(0x3FFE, 0xE03995F0F4FF5B70),
        L::from_bits(0xBFBD, 0x9A68C72A11BCDFED),
    ),
    (
        L::from_bits(0x3FFE, 0xE18D13EE805A4DE3),
        L::from_bits(0x3FBD, 0x84944C47AC1BF62B),
    ),
    (
        L::from_bits(0x3FFE, 0xE2E1CB1CA47D25B3),
        L::from_bits(0xBFBA, 0xB38F323CBE58AF0D),
    ),
    (
        L::from_bits(0x3FFE, 0xE437BDBF5254459C),
        L::from_bits(0x3FBD, 0x9A74B235CD0A8F0D),
    ),
    (
        L::from_bits(0x3FFE, 0xE58EEE20CB7B6C15),
        L::from_bits(0xBFBD, 0x88B03C7785A7624C),
    ),
    (
        L::from_bits(0x3FFE, 0xE6E75E91B9CCA552),
        L::from_bits(0xBFBD, 0xE464929B67474641),
    ),
    (
        L::from_bits(0x3FFE, 0xE99C090536ECE983),
        L::from_bits(0x3FBC, 0xCEB1F67AEEA294F1),
    ),
    (
        L::from_bits(0x3FFE, 0xEAF847C9FCC4492B),
        L::from_bits(0x3FBA, 0xD2C84965D2BBD614),
    ),
    (
        L::from_bits(0x3FFE, 0xEC55D022D80E3D28),
        L::from_bits(0xBFB9, 0x9A2243694C4ED4DB),
    ),
    (
        L::from_bits(0x3FFE, 0xEDB4A481ECA5C376),
        L::from_bits(0x3FBD, 0xBA6195D4134BE925),
    ),
    (
        L::from_bits(0x3FFE, 0xEF14C7605D60654C),
        L::from_bits(0x3FBC, 0xC22D15199B7A3E65),
    ),
    (
        L::from_bits(0x3FFE, 0xF0763B3E66D6302F),
        L::from_bits(0x3FBB, 0xCBE51121A93C2683),
    ),
    (
        L::from_bits(0x3FFE, 0xF1D902A37AAA5086),
        L::from_bits(0xBFBD, 0xF8F869E63B882858),
    ),
    (
        L::from_bits(0x3FFE, 0xF33D201E5B5735D0),
        L::from_bits(0xBFBD, 0xA0E3B50F7A10AE3E),
    ),
    (
        L::from_bits(0x3FFE, 0xF4A2964538813C67),
        L::from_bits(0x3FBD, 0xC9F90F69483EF6DF),
    ),
    (
        L::from_bits(0x3FFE, 0xF60967B5CBD2ECF0),
        L::from_bits(0xBFBB, 0x9D2DC0FE24A3D4A2),
    ),
    (
        L::from_bits(0x3FFE, 0xF77197157665F689),
        L::from_bits(0x3FBC, 0xFD75F60AD9715A19),
    ),
    (
        L::from_bits(0x3FFE, 0xF8DB27115EBC1E66),
        L::from_bits(0x3FBD, 0xC8BF847459627A7D),
    ),
    (
        L::from_bits(0x3FFE, 0xFA461A5E8F4B759D),
        L::from_bits(0x3FBD, 0xC8EC0EF73F7A835D),
    ),
    (
        L::from_bits(0x3FFE, 0xFBB273BA15A13CED),
        L::from_bits(0x3FBB, 0x97CB26DDDD6F7274),
    ),
    (
        L::from_bits(0x3FFE, 0xFD2035E9221EF5D0),
        L::from_bits(0x3FBA, 0xE3909FFD0D61777C),
    ),
    (
        L::from_bits(0x3FFE, 0xFE8F63B92855388B),
        L::from_bits(0x3FBD, 0xA4C85B6DA0F78A44),
    ),
];
const LOG2E_HI: L = L::from_bits(0x3FFF, 0xB8AA3B295C17F0BC);
const LOG2E_LO: L = L::from_bits(0xBFBE, 0x82F0025F2DC582EE);
const LOG2_10D_HI: L = L::from_bits(0x4000, 0xD49A784BCD1B8AFE);
const LOG2_10D_LO: L = L::from_bits(0x3FBF, 0x9257EDFE9B5FB69A);

/// `a + b` as `(s, e)`, `s = rn(a + b)`, `s + e = a + b` exactly (Knuth).
fn two_sum(a: L, b: L) -> (L, L) {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    (s, e)
}

/// `a` as `hi + lo`, each with at most 32 significant bits (Veltkamp).
fn split32(a: L) -> (L, L) {
    let c = a * ld(4_294_967_297.0); // 2^32 + 1
    let hi = c - (c - a);
    (hi, a - hi)
}

/// `a * b` as `(p, e)`, `p = rn(a b)`, `p + e = a b` exactly (Dekker).
fn two_prod(a: L, b: L) -> (L, L) {
    let p = a * b;
    let (ah, al) = split32(a);
    let (bh, bl) = split32(b);
    let e = ((ah * bh - p) + ah * bl + al * bh) + al * bl;
    (p, e)
}

/// `log2(x)` for a finite `x > 0`, as `hi + lo` to about 80 bits.
///
/// `x = 2^e m`, `m` in `[1, 2)`; `m` is multiplied by a 9-bit reciprocal
/// `inv` of the middle of its 128th of `[1, 2)`, which leaves `r = m inv - 1`
/// with `|r| < 1/128`, computed exactly as `rh + rl` because both halves of
/// `m` times `inv` are exact; then `log2(m) = -log2(inv) + log2(1 + r)`,
/// the first from the table, the second a series.
fn log2_dd(x: L) -> (L, L) {
    let mut x = x;
    let mut e = i64::from(x.biased_exponent()) - 0x3FFF;
    if x.biased_exponent() == 0 {
        // Subnormal: normalise by 2^64 first.
        x = x * L::from_bits(0x3FFF + 64, 1 << 63);
        e = i64::from(x.biased_exponent()) - 0x3FFF - 64;
    }
    let m = L::from_bits(0x3FFF, x.significand);
    #[allow(clippy::cast_possible_truncation)]
    let k = ((x.significand >> 56) & 0x7F) as usize;
    let inv = LOG2_INV.get(k).copied().unwrap_or(ONE);
    let (t_hi, t_lo) = LOG2_TAB.get(k).copied().unwrap_or((ZERO, ZERO));
    // r = m inv - 1, exactly, as rh + rl.
    let (mh, ml) = split32(m);
    let (rh, rl) = two_sum(mh * inv - ONE, ml * inv);
    // ln(1 + r) = r - r^2/2 + r^3/3 - ...: the square exactly, the rest,
    // below 2^-20 relative, in plain long double.
    let (sq_hi, sq_lo) = two_prod(rh, rh);
    let half = ld(0.5);
    // r^3/3 - r^4/4 + ... - r^12/12, Horner in rh.
    let mut tail = ZERO;
    for n in (3..=12).rev() {
        let c = ONE / L::from_i64(n);
        let c = if n % 2 == 0 { -c } else { c };
        tail = c + rh * tail;
    }
    tail = tail * rh * rh * rh;
    let (lh, l0) = two_sum(rh, -(sq_hi * half));
    let ll = l0 + rl - sq_lo * half - rh * rl + tail;
    // To base 2: (lh + ll)(log2 e) as a pair.
    let (ph, p0) = two_prod(lh, LOG2E_HI);
    let pl = p0 + lh * LOG2E_LO + ll * LOG2E_HI;
    // e + table + series.
    let (s1, e1) = two_sum(L::from_i64(e), t_hi);
    let (s2, e2) = two_sum(s1, ph);
    let lo = e1 + e2 + t_lo + pl;
    two_sum(s2, lo)
}

/// `2^(hi + lo)`, `|lo| <= ulp(hi)`: the integer nearest `hi` as an exact
/// scale, the rest by `f2xm1`. Overflows or underflows as `2^hi` must.
fn exp2_dd(hi: L, lo: L) -> L {
    if hi > ld(16_384.0) {
        return overflow();
    }
    if hi < ld(-16_446.0) {
        return underflow();
    }
    let n = hi.round_int();
    let f = (hi - n) + lo;
    let p = f.f2xm1() + ONE;
    #[allow(clippy::cast_possible_truncation)]
    scalbnl_raw(p, n.to_i64_rint() as i32)
}

/// `x^y` for a finite `x > 0` and a finite `y`, to about an ulp.
fn pow_positive(x: L, y: L) -> L {
    let (lh, ll) = log2_dd(x);
    let (th, t0) = two_prod(y, lh);
    let tl = t0 + y * ll;
    let (hi, lo) = two_sum(th, tl);
    exp2_dd(hi, lo)
}

/// 2^10000, whose square overflows; and 2^-10000, whose square underflows.
const HUGE_10000: L = L::from_bits(0x3FFF + 10000, 1 << 63);
const TINY_10000: L = L::from_bits(0x3FFF - 10000, 1 << 63);

fn overflow() -> L {
    let h = core::hint::black_box(HUGE_10000);
    h * h
}

fn underflow() -> L {
    let t = core::hint::black_box(TINY_10000);
    t * t
}

/// `x^n` for `x > 0` and an integer `n`, by squaring (Cephes' `powil`).
fn powil(x: L, nn: i32) -> L {
    if nn == 0 {
        return ONE;
    }
    let mut sign = nn >= 0;
    let mut n = nn.unsigned_abs();
    // Overflow detection, from the approximate logarithm of the answer.
    let (s0, lx) = frexpl(x);
    let e = i64::from(lx - 1) * i64::from(n);
    let s = if e == 0 || !(-64..=64).contains(&e) {
        let s = (s0 - POW_SQRTH) / (s0 + POW_SQRTH);
        (POW_POWIL_K * s - ld(0.5) + L::from_i64(i64::from(lx)))
            * L::from_i64(i64::from(nn))
            * POW_LOGE2L
    } else {
        POW_LOGE2L * L::from_i64(e)
    };
    if s > POW_MAXLOGL {
        return overflow();
    }
    if s < POW_MINLOGL {
        return underflow();
    }
    let mut x = x;
    // A tiny subnormal answer: less accurate, as roundoff in 1/x is
    // amplified.
    if s < -POW_MAXLOGL + ld(2.0) {
        x = ONE / x;
        sign = !sign;
    }
    let mut y = if n & 1 != 0 { x } else { ONE };
    let mut ww = x;
    n >>= 1;
    while n != 0 {
        ww = ww * ww;
        if n & 1 != 0 {
            y = y * ww;
        }
        n >>= 1;
    }
    if sign { y } else { ONE / y }
}

/// glibc's `w_pow_template.c`: a non-finite answer from finite arguments is
/// `EDOM` if it is a NaN and `ERANGE` otherwise -- `powl(0, y<0)`'s pole or
/// an overflow; a zero from a finite nonzero base and a finite power is an
/// underflow, `ERANGE`. The pole and the NaN are the arguments' doing and
/// are reported here; the overflow and the underflow are [`pow_range`]'s,
/// through [`ranged`].
fn pow_errno(x: L, y: L, z: L) -> L {
    if x.is_finite() && y.is_finite() {
        if z.is_nan() {
            set(errno::EDOM);
        } else if x.is_zero() && y < ZERO {
            set(errno::ERANGE);
        }
    }
    z
}

/// [`Range`] for `powl`: an infinity from a finite nonzero base and a finite
/// power overflowed, and a zero from them underflowed.
fn pow_range(x: L, y: L, z: L) -> Range {
    if !x.is_finite() || !y.is_finite() || x.is_zero() {
        Range::Within
    } else {
        overflow_or_underflow(true, z)
    }
}

/// `x` to the power `y`: computed to nearest, since the accurate path
/// carries `y log2 x` in exact sums and products, which are exact only
/// there; the pole and the domain error by `pow_errno`, an overflow or
/// underflow by [`ranged`].
#[must_use]
pub fn powl(x: L, y: L) -> L {
    let z = ranged(
        (x, y),
        |a| crate::fenv::in_nearest_x87(a, |(x, y)| powl_raw(x, y)),
        |z| pow_range(x, y, z),
    );
    pow_errno(x, y, z)
}

/// musl's ld80 `powl`.
#[allow(clippy::too_many_lines)]
fn powl_raw(x: L, y: L) -> L {
    if x.is_nan() {
        if !y.is_nan() && y.is_zero() {
            return ONE;
        }
        return x;
    }
    if y.is_nan() {
        if x == ONE {
            return ONE;
        }
        return y;
    }
    if x == ONE || y.is_zero() {
        return ONE;
    }
    if y == ONE {
        return x;
    }
    // |y| beyond 2 (-LDBL_MIN_EXP + LDBL_MANT_DIG + 1) / LDBL_EPSILON: not an
    // odd integer, and x^y over- or underflows unless |x| == 1.
    let lim = ld(f64::from(2 * (16381 + 64 + 1))) * L::from_bits(0x3FFF + 63, 1 << 63);
    if y.abs() > lim {
        if x == -ONE {
            return ONE;
        }
        let big_x = x > ONE || x < -ONE;
        if y.is_infinite() {
            return if big_x != y.is_sign_negative() {
                L::INFINITY
            } else {
                ZERO
            };
        }
        return if big_x == (y > ZERO) {
            overflow()
        } else {
            underflow()
        };
    }
    if x == L::INFINITY {
        return if y > ZERO { L::INFINITY } else { ZERO };
    }
    let w = floorl(y);
    let iyflg = w == y;
    let yoddint = iyflg && floorl(ld(0.5) * y.abs()) != ld(0.5) * w.abs();
    if x == -L::INFINITY {
        if y > ZERO {
            return if yoddint { -L::INFINITY } else { L::INFINITY };
        }
        return if yoddint { -ZERO } else { ZERO };
    }
    let mut x = x;
    let mut nflg = false;
    if x <= ZERO {
        if x.is_zero() {
            if y < ZERO {
                // (+-0)^negative: a pole, divide-by-zero.
                let inf = ONE / core::hint::black_box(ZERO);
                return if x.is_sign_negative() && yoddint {
                    -inf
                } else {
                    inf
                };
            }
            return if x.is_sign_negative() && yoddint {
                -ZERO
            } else {
                ZERO
            };
        }
        if !iyflg {
            // (x < 0)^(non-integer)
            return (x - x) / (x - x);
        }
        nflg = yoddint;
        x = -x;
    }
    // A small integer power by multiplication, as glibc does for |y| < 4:
    // exact whenever every partial product is.
    if iyflg && y.abs() < ld(4.0) {
        #[allow(clippy::cast_possible_truncation)]
        let w = powil(x, y.to_i64_rint() as i32);
        return if nflg { -w } else { w };
    }
    let z = pow_positive(x, y);
    if nflg { -z } else { z }
}

/// 10^x: musl's ld80 `exp10l` while the integer part is below 16 -- an
/// exact power of ten times `2^(frac(x) log2 10)` -- and beyond it
/// `2^(x log2 10)` with the product carried to about 80 bits, as the
/// accurate `powl` below does (musl's `powl(10, x)` there is its
/// inaccurate one).
#[must_use]
pub fn exp10l(x: L) -> L {
    // To nearest: its double-long-double products are exact only there.
    exp_like(x, |x| crate::fenv::in_nearest_x87(x, exp10l_raw))
}

fn exp10l_raw(x: L) -> L {
    let (y, n) = modfl(x);
    // |n| < 16, without raising invalid on NaN.
    if n.biased_exponent() < 0x3FFF + 4 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k = (n.to_i64_rint() + 15) as usize;
        let p = P10.get(k).copied().unwrap_or(ONE);
        if y.is_zero() {
            return p;
        }
        let y = exp2l_raw(L::LOG2_10 * y);
        return y * p;
    }
    if !x.is_finite() {
        // NaN, or 10^+-inf.
        return if x.is_nan() {
            x + x
        } else if x.is_sign_negative() {
            ZERO
        } else {
            x
        };
    }
    // 10^x overflows for every x above about 4932 and underflows to zero
    // below about -4951; answering those here also keeps the exact product
    // below from overflowing its split.
    if x > ld(5000.0) {
        return overflow();
    }
    if x < ld(-5000.0) {
        return underflow();
    }
    let (th, t0) = two_prod(x, LOG2_10D_HI);
    let (hi, lo) = two_sum(th, t0 + x * LOG2_10D_LO);
    exp2_dd(hi, lo)
}

/// 10^-15 .. 10^15, each rounded to the long double nearest.
const P10: [L; 31] = [
    L::from_bits(0x3FCD, 0x901D_7CF7_3AB0_ACD9),
    L::from_bits(0x3FD0, 0xB424_DC35_095C_D80F),
    L::from_bits(0x3FD3, 0xE12E_1342_4BB4_0E13),
    L::from_bits(0x3FD7, 0x8CBC_CC09_6F50_88CC),
    L::from_bits(0x3FDA, 0xAFEB_FF0B_CB24_AAFF),
    L::from_bits(0x3FDD, 0xDBE6_FECE_BDED_D5BF),
    L::from_bits(0x3FE1, 0x8970_5F41_36B4_A597),
    L::from_bits(0x3FE4, 0xABCC_7711_8461_CEFD),
    L::from_bits(0x3FE7, 0xD6BF_94D5_E57A_42BC),
    L::from_bits(0x3FEB, 0x8637_BD05_AF6C_69B6),
    L::from_bits(0x3FEE, 0xA7C5_AC47_1B47_8423),
    L::from_bits(0x3FF1, 0xD1B7_1758_E219_652C),
    L::from_bits(0x3FF5, 0x8312_6E97_8D4F_DF3B),
    L::from_bits(0x3FF8, 0xA3D7_0A3D_70A3_D70A),
    L::from_bits(0x3FFB, 0xCCCC_CCCC_CCCC_CCCD),
    L::from_bits(0x3FFF, 0x8000_0000_0000_0000),
    L::from_bits(0x4002, 0xA000_0000_0000_0000),
    L::from_bits(0x4005, 0xC800_0000_0000_0000),
    L::from_bits(0x4008, 0xFA00_0000_0000_0000),
    L::from_bits(0x400C, 0x9C40_0000_0000_0000),
    L::from_bits(0x400F, 0xC350_0000_0000_0000),
    L::from_bits(0x4012, 0xF424_0000_0000_0000),
    L::from_bits(0x4016, 0x9896_8000_0000_0000),
    L::from_bits(0x4019, 0xBEBC_2000_0000_0000),
    L::from_bits(0x401C, 0xEE6B_2800_0000_0000),
    L::from_bits(0x4020, 0x9502_F900_0000_0000),
    L::from_bits(0x4023, 0xBA43_B740_0000_0000),
    L::from_bits(0x4026, 0xE8D4_A510_0000_0000),
    L::from_bits(0x402A, 0x9184_E72A_0000_0000),
    L::from_bits(0x402D, 0xB5E6_20F4_8000_0000),
    L::from_bits(0x4030, 0xE35F_A931_A000_0000),
];

// ===========================================================================
// Error function and gamma functions: musl's ld80 erfl.c, lgammal.c,
// tgammal.c (FreeBSD's and Cephes' coefficients)
// ===========================================================================

const ERF_ERX: L = L::from_bits(0x3FFE, 0xD8560B0000000000); // 0.845062911510467529296875L
const ERF_EFX8: L = L::from_bits(0x3FFF, 0x8375D410A6DB446C); // 1.0270333367641005911692712249723613735048E0L
const ERF_PP: [L; 6] = [
    L::from_bits(0x4013, 0x890DFACEC680CB0C), // 1.122751350964552113068262337278335028553E6L
    L::from_bits(0xC014, 0xAB6B55353EE251F9), // -2.808533301997696164408397079650699163276E6L
    L::from_bits(0xC011, 0xA1D511887DC5E424), // -3.314325479115357458197119660818768924100E5L
    L::from_bits(0xC00F, 0x85C36C1D991D946E), // -6.848684465326256109712135497895525446398E4L
    L::from_bits(0xC00A, 0xA61D154777EB9519), // -2.657817695110739185591505062971929859314E3L
    L::from_bits(0xC006, 0xA587F1999B164FE8), // -1.655310302737837556654146291646499062882E2L
];
const ERF_QQ: [L; 6] = [
    L::from_bits(0x4016, 0x8572745F3EF624EE), // 8.745588372054466262548908189000448124232E6L
    L::from_bits(0x4014, 0xE4A3D90F25C1EEE9), // 3.746038264792471129367533128637019611485E6L
    L::from_bits(0x4012, 0xAC84BE0D9554997B), // 7.066358783162407559861156173539693900031E5L
    L::from_bits(0x400F, 0x917CA49D3A9B4669), // 7.448928604824620999413120955705448117056E4L
    L::from_bits(0x400B, 0x8CFCAC013C37575E), // 4.511583986730994111992253980546131408924E3L
    L::from_bits(0x4006, 0x88E3EA4B462EBBD9), // 1.368902937933296323345610240009071254014E2L
];
const ERF_PA: [L; 8] = [
    L::from_bits(0xBFFF, 0x89D991639982CCAF), // -1.076952146179812072156734957705102256059E0L
    L::from_bits(0x4006, 0xBC7B434EA78AEFB4), // 1.884814957770385593365179835059971587220E2L
    L::from_bits(0xC004, 0xD590EFCBD7892BB3), // -5.339153975012804282890066622962070115606E1L
    L::from_bits(0x4004, 0xB16FB9B1509C0C90), // 4.435910679869176625928504532109635632618E1L
    L::from_bits(0x4003, 0x86A855EFABD2F1A5), // 1.683219516032328828278557309642929135179E1L
    L::from_bits(0xC000, 0x970E1DE3B589EC74), // -2.360236618396952560064259585299045804293E0L
    L::from_bits(0x3FFF, 0xED15DFCC1E1FA493), // 1.852230047861891953244413872297940938041E0L
    L::from_bits(0x3FFB, 0xC068D41797354859), // 9.394994446747752308256773044667843200719E-2L
];
const ERF_QA: [L; 7] = [
    L::from_bits(0x4007, 0xE3F6935D7EB50540), // 4.559263722294508998149925774781887811255E2L
    L::from_bits(0x4007, 0xA47663109BAFACB2), // 3.289248982200800575749795055149780689738E2L
    L::from_bits(0x4007, 0x8E4DB5574A8D0A2C), // 2.846070965875643009598627918383314457912E2L
    L::from_bits(0x4006, 0x8BDF204103DF5006), // 1.398715859064535039433275722017479994465E2L
    L::from_bits(0x4004, 0xF2685A65A8D2812C), // 6.060190733759793706299079050985358190726E1L
    L::from_bits(0x4003, 0xA64BAFFEC56E6F95), // 2.078695677795422351040502569964299664233E1L
    L::from_bits(0x4001, 0x94854B0AA1806CA0), // 4.641271134150895940966798357442234498546E0L
];
const ERF_RA: [L; 9] = [
    L::from_bits(0x3FFC, 0x8BA1147F23483020), // 1.363566591833846324191000679620738857234E-1L
    L::from_bits(0x4002, 0xA2E99A0AEEE04E8C), // 1.018203167219873573808450274314658434507E1L
    L::from_bits(0x4006, 0xBA3C66512680507E), // 1.862359362334248675526472871224778045594E2L
    L::from_bits(0x4009, 0xB073EC3E0C625EAF), // 1.411622588180721285284945138667933330348E3L
    L::from_bits(0x400B, 0x9F044EC3FB200D87), // 5.088538459741511988784440103218342840478E3L
    L::from_bits(0x400C, 0x8B81019759F3B6EA), // 8.928251553922176506858267311750789273656E3L
    L::from_bits(0x400B, 0xE3037CEDA5477EA6), // 7.264436000148052545243018622742770549982E3L
    L::from_bits(0x400A, 0x9537E11D629D5E93), // 2.387492459664548651671894725748959751119E3L
    L::from_bits(0x4006, 0xDE17776039C08D90), // 2.220916652813908085449221282808458466556E2L
];
const ERF_SA: [L; 9] = [
    L::from_bits(0xC002, 0xDD28548B26F49C21), // -1.382234625202480685182526402169222331847E1L
    L::from_bits(0xC007, 0xA5C82D562A7A71C7), // -3.315638835627950255832519203687435946482E2L
    L::from_bits(0xC00A, 0xB851FF714D683071), // -2.949124863912936259747237164260785326692E3L
    L::from_bits(0xC00C, 0xC2C8E24B636B5DB4), // -1.246622099070875940506391433635999693661E4L
    L::from_bits(0xC00D, 0xD0D5988E04C9308E), // -2.673079795851665428695842853070996219632E4L
    L::from_bits(0xC00D, 0xE105654EC58FFCF7), // -2.880269786660559337358397106518918220991E4L
    L::from_bits(0xC00C, 0xE2A80256FBB4CDAD), // -1.450600228493968044773354186390390823713E4L
    L::from_bits(0xC00A, 0xB3A8A2BD1AE5892A), // -2.874539731125893533960680525192064277816E3L
    L::from_bits(0xC006, 0x8C396054B1AB12AE), // -1.402241261419067750237395034116942296027E2L
];
const ERF_RB: [L; 8] = [
    L::from_bits(0xBFF0, 0xCC3ECBC79CC360D1), // -4.869587348270494309550558460786501252369E-5L
    L::from_bits(0xBFF7, 0x840FC34A1215833D), // -4.030199390527997378549161722412466959403E-3L
    L::from_bits(0xBFFB, 0xC137900C35C6FA33), // -9.434425866377037610206443566288917589122E-2L
    L::from_bits(0xBFFE, 0xEE91368B082A3D37), // -9.319032754357658601200655161585539404155E-1L
    L::from_bits(0xC001, 0x88C2DF6AD5D3AA0D), // -4.273788174307459947350256581445442062291E0L
    L::from_bits(0xC002, 0x8D7A050450A2CC9B), // -8.842289940696150508373541814064198259278E0L
    L::from_bits(0xC001, 0xE23702E61492A1B2), // -7.069215249419887403187988144752613025255E0L
    L::from_bits(0xBFFF, 0xB35B767B1FAF5426), // -1.401228723639514787920274427443330704764E0L
];
const ERF_SB: [L; 7] = [
    L::from_bits(0x3FF7, 0xA1C04ED159F6AE68), // 4.936254964107175160157544545879293019085E-3L
    L::from_bits(0x3FFC, 0xA225643BF00BB255), // 1.583457624037795744377163924895349412015E-1L
    L::from_bits(0x3FFF, 0xECE2088CB3DEBC0A), // 1.850647991850328356622940552450636420484E0L
    L::from_bits(0x4002, 0x9ED77F375E7AF590), // 9.927611557279019463768050710008450625415E0L
    L::from_bits(0x4003, 0xCA888BA1BCAABB46), // 2.531667257649436709617165336779212114570E1L
    L::from_bits(0x4003, 0xE5948A03597C3A41), // 2.869752886406743386458304052862814690045E1L
    L::from_bits(0x4002, 0xBD2128334D0429EA), // 1.182059497870819562441683560749192539345E1L
];
const ERF_RC: [L; 6] = [
    L::from_bits(0xBFF1, 0xAE0E3B7F13E29D19), // -8.299617545269701963973537248996670806850E-5L
    L::from_bits(0xBFF7, 0xCC992C826973D624), // -6.243845685115818513578933902532056244108E-3L
    L::from_bits(0xBFFB, 0xE9D03DEA0628E5B6), // -1.141667210620380223113693474478394397230E-1L
    L::from_bits(0xBFFE, 0xC08BE0F3198D261E), // -7.521343797212024245375240432734425789409E-1L
    L::from_bits(0xBFFF, 0xE1F611A68108769A), // -1.765321928311155824664963633786967602934E0L
    L::from_bits(0xBFFF, 0x83C37E35AEFE08A5), // -1.029403473103215800456761180695263439188E0L
];
const ERF_SC: [L; 5] = [
    L::from_bits(0x3FF8, 0x89D7B4591D2BD98B), // 8.413244363014929493035952542677768808601E-3L
    L::from_bits(0x3FFC, 0xD377BBB27F6AFCD4), // 2.065114333816877479753334599639158060979E-1L
    L::from_bits(0x3FFF, 0xD1CCE147F28CFE7F), // 1.639064941530797583766364412782135680148E0L
    L::from_bits(0x4001, 0x9DFA2BCCE78B3201), // 4.936788463787115555582319302981666347450E0L
    L::from_bits(0x4001, 0xA02A6A7B20F65C06), // 5.005177727208955487404729933261347679090E0L
];

/// `P[0] x^n + P[1] x^(n-1) + ... + P[n]` (Cephes' `polevl`).
fn polevl(x: L, p: &[L]) -> L {
    let mut it = p.iter();
    let mut y = it.next().copied().unwrap_or(ZERO);
    for &c in it {
        y = y * x + c;
    }
    y
}

/// `c[0] + x (c[1] + x (c[2] + ...))`: the coefficients low order first,
/// as musl writes its nested polynomials out.
fn horner_l(x: L, c: &[L]) -> L {
    let mut acc = ZERO;
    for &k in c.iter().rev() {
        acc = k + x * acc;
    }
    acc
}

/// `c[0] + x (c[1] + ... + x (c[n-1] + x))`: [`horner_l`] with a leading 1.
fn horner1_l(x: L, c: &[L]) -> L {
    let mut acc = ONE;
    for &k in c.iter().rev() {
        acc = k + x * acc;
    }
    acc
}

/// The key musl's ld80 code compares against: the exponent word with the
/// top 16 bits of the significand.
fn ix_of(x: L) -> u32 {
    (u32::from(x.sign_exp & 0x7FFF) << 16) | ((x.significand >> 48) as u32)
}

/// `erfc(|x|)` for `0.84375 <= |x| < 1.25` (musl's `erfc1`).
fn erfc1(x: L) -> L {
    let s = x.abs() - ONE;
    let p = horner_l(s, &ERF_PA);
    let q = horner1_l(s, &ERF_QA);
    ONE - ERF_ERX - p / q
}

/// `erfc(|x|)` for `0.84375 <= |x| < 107` (musl's `erfc2`).
fn erfc2(ix: u32, x: L) -> L {
    if ix < 0x3FFF_A000 {
        // 0.84375 <= |x| < 1.25
        return erfc1(x);
    }
    let x = x.abs();
    let s = ONE / (x * x);
    let (r, sq) = if ix < 0x4000_B6DB {
        // 1.25 <= |x| < 2.857 ~ 1/.35
        (horner_l(s, &ERF_RA), horner1_l(s, &ERF_SA))
    } else if ix < 0x4001_D555 {
        // 2.857 <= |x| < 6.6666259765625
        (horner_l(s, &ERF_RB), horner1_l(s, &ERF_SB))
    } else {
        // 6.666 <= |x| < 107 (erfc only)
        (horner_l(s, &ERF_RC), horner1_l(s, &ERF_SC))
    };
    // z = x with its low 40 significand bits cleared, so z*z is exact.
    let z = L::from_bits(x.sign_exp, x.significand & (u64::MAX << 40));
    expl_raw(-z * z - ld(0.5625)) * expl_raw((z - x) * (z + x) + r / sq) / x
}

/// The error function.
#[must_use]
pub fn erfl(x: L) -> L {
    let ix = ix_of(x);
    let neg = x.is_sign_negative();
    if ix >= 0x7FFF_0000 {
        // erf(nan) = nan, erf(+-inf) = +-1
        let s = if neg { -ONE } else { ONE };
        return s + ONE / x;
    }
    if ix < 0x3FFE_D800 {
        // |x| < 0.84375
        if ix < 0x3FDE_8000 {
            // |x| < 2^-33: avoid underflow
            return ld(0.125) * (ld(8.0) * x + ERF_EFX8 * x);
        }
        let z = x * x;
        let r = horner_l(z, &ERF_PP);
        let s = horner1_l(z, &ERF_QQ);
        let y = r / s;
        return x + x * y;
    }
    let y = if ix < 0x4001_D555 {
        // |x| < 6.6666259765625
        ONE - erfc2(ix, x)
    } else {
        ONE - L::from_bits(0x0001, 1 << 63) // 1 - 2^-16382
    };
    if neg { -y } else { y }
}

/// The complementary error function, `1 - erf(x)`; `ERANGE` when it
/// underflows to zero for a finite positive `x` (glibc's).
#[must_use]
pub fn erfcl(x: L) -> L {
    ranged(x, erfcl_raw, |r| {
        underflow_only(r.is_zero() && x > ZERO && x.is_finite())
    })
}

fn erfcl_raw(x: L) -> L {
    let ix = ix_of(x);
    let neg = x.is_sign_negative();
    if ix >= 0x7FFF_0000 {
        // erfc(nan) = nan, erfc(+-inf) = 0, 2
        let s = if neg { ld(2.0) } else { ZERO };
        return s + ONE / x;
    }
    if ix < 0x3FFE_D800 {
        // |x| < 0.84375
        if ix < 0x3FBE_0000 {
            // |x| < 2^-65
            return ONE - x;
        }
        let z = x * x;
        let r = horner_l(z, &ERF_PP);
        let s = horner1_l(z, &ERF_QQ);
        let y = r / s;
        if ix < 0x3FFD_8000 {
            // x < 1/4
            return ONE - (x + x * y);
        }
        return ld(0.5) - (x - ld(0.5) + x * y);
    }
    if ix < 0x4005_D600 {
        // |x| < 107
        return if neg {
            ld(2.0) - erfc2(ix, x)
        } else {
            erfc2(ix, x)
        };
    }
    let y = L::from_bits(0x0001, 1 << 63); // 2^-16382
    if neg { ld(2.0) - y } else { y * y }
}

const LG_PI: L = L::from_bits(0x4000, 0xC90FDAA22168C235); // 3.14159265358979323846264L
const LG_A0: L = L::from_bits(0xC008, 0x9E94C7302246C2A8); // -6.343246574721079391729402781192128239938E2L
const LG_A1: L = L::from_bits(0x4009, 0xE811ED79A7063DDF); // 1.856560238672465796768677717168371401378E3L
const LG_A2: L = L::from_bits(0x400A, 0x964BBAC9559EAC31); // 2.404733102163746263689288466865843408429E3L
const LG_A3: L = L::from_bits(0x4008, 0xDC1ACEEC4B3BA538); // 8.804188795790383497379532868917517596322E2L
const LG_A4: L = L::from_bits(0x4005, 0xE31280572629A7F8); // 1.135361354097447729740103745999661157426E2L
const LG_A5: L = L::from_bits(0x4000, 0xF115D0E13AEA5AAC); // 3.766956539107615557608581581190400021285E0L
const LG_B0: L = L::from_bits(0x400C, 0x805BE51545C76426); // 8.214973713960928795704317259806842490498E3L
const LG_B1: L = L::from_bits(0x400C, 0xA05DBD87D12E640B); // 1.026343508841367384879065363925870888012E4L
const LG_B2: L = L::from_bits(0x400B, 0x8E4AB3272A55A21D); // 4.553337477045763320522762343132210919277E3L
const LG_B3: L = L::from_bits(0x4008, 0xD4ACA5204E78D8C0); // 8.506975785032585797446253359230031874803E2L
const LG_B4: L = L::from_bits(0x4004, 0xF1B2AA9F4861B5CB); // 6.042447899703295436820744186992189445813E1L
const LG_TC: L = L::from_bits(0x3FFF, 0xBB16C31AB5F1FB71); // 1.4616321449683623412626595423257213284682E0L
const LG_TF: L = L::from_bits(0xBFFB, 0xF8CDCDE61C521000); // -1.2148629053584961146050602565082954242826E-1
const LG_TT: L = L::from_bits(0x3FC4, 0xF84AE446AD360AC8); // 3.3649914684731379602768989080467587736363E-18L
const LG_G0: L = L::from_bits(0x3FC5, 0x867F0E7C29340760); // 3.645529916721223331888305293534095553827E-18L
const LG_G1: L = L::from_bits(0x400B, 0xA0353CB55C121785); // 5.126654642791082497002594216163574795690E3L
const LG_G2: L = L::from_bits(0x400C, 0x89F26A0FC9F11345); // 8.828603575854624811911631336122070070327E3L
const LG_G3: L = L::from_bits(0x400B, 0xAAC17DCD67444A43); // 5.464186426932117031234820886525701595203E3L
const LG_G4: L = L::from_bits(0x4009, 0xB5EDAD4A2B6AF60D); // 1.455427403530884193180776558102868592293E3L
const LG_G5: L = L::from_bits(0x4006, 0x9A2C6D7DA4A7143C); // 1.541735456969245924860307497029155838446E2L
const LG_G6: L = L::from_bits(0x4001, 0x8ABC66E1056EBB97); // 4.335498275274822298341872707453445815118E0L
const LG_H0: L = L::from_bits(0x400C, 0xA58F65AF2D6352AF); // 1.059584930106085509696730443974495979641E4L
const LG_H1: L = L::from_bits(0x400D, 0xA7CE6EDDA9979471); // 2.147921653490043010629481226937850618860E4L
const LG_H2: L = L::from_bits(0x400D, 0x805C4B9F648BE3D4); // 1.643014770044524804175197151958100656728E4L
const LG_H3: L = L::from_bits(0x400B, 0xB7682D0BD0041DB4); // 5.869021995186925517228323497501767586078E3L
const LG_H4: L = L::from_bits(0x4008, 0xF41B2AA4D08C27AA); // 9.764244777714344488787381271643502742293E2L
const LG_H5: L = L::from_bits(0x4005, 0x80D986849A094970); // 6.442485441570592541741092969581997002349E1L
const LG_U0: L = L::from_bits(0xC005, 0xB1B96F0070C4A41B); // -8.886217500092090678492242071879342025627E1L
const LG_U1: L = L::from_bits(0x4008, 0xAB00B4302BB3CBF1); // 6.840109978129177639438792958320783599310E2L
const LG_U2: L = L::from_bits(0x4009, 0xFF54090C5581BF2A); // 2.042626104514127267855588786511809932433E3L
const LG_U3: L = L::from_bits(0x4009, 0xEEF72A378D76FD0C); // 1.911723903442667422201651063009856064275E3L
const LG_U4: L = L::from_bits(0x4008, 0xBA2D37BF660E26EB); // 7.447065275665887457628865263491667767695E2L
const LG_U5: L = L::from_bits(0x4005, 0xE2738851DB576BEE); // 1.132256494121790736268471016493103952637E2L
const LG_U6: L = L::from_bits(0x4001, 0x8F80321770681672); // 4.484398885516614191003094714505960972894E0L
const LG_V0: L = L::from_bits(0x4009, 0x8FDA96EE56155B39); // 1.150830924194461522996462401210374632929E3L
const LG_V1: L = L::from_bits(0x400A, 0xD47B13801C9ABE6B); // 3.399692260848747447377972081399737098610E3L
const LG_V2: L = L::from_bits(0x400A, 0xECAA1B7760BA7C84); // 3.786631705644460255229513563657226008015E3L
const LG_V3: L = L::from_bits(0x4009, 0xF5CE67685BED2E20); // 1.966450123004478374557778781564114347876E3L
const LG_V4: L = L::from_bits(0x4007, 0xED116565A2F267A3); // 4.741359068914069299837355438370682773122E2L
const LG_V5: L = L::from_bits(0x4004, 0xB45C0DD3D2AA7F6A); // 4.508989649747184050907206782117647852364E1L
const LG_S0: L = L::from_bits(0x4013, 0xB194321B770C93B4); // 1.454726263410661942989109455292824853344E6L
const LG_S1: L = L::from_bits(0xC014, 0xEE1FD18F72CBB4CD); // -3.901428390086348447890408306153378922752E6L
const LG_S2: L = L::from_bits(0xC015, 0xC89C01657BB2F73A); // -6.573568698209374121847873064292963089438E6L
const LG_S3: L = L::from_bits(0xC014, 0xCA943F86A403F6BE); // -3.319055881485044417245964508099095984643E6L
const LG_S4: L = L::from_bits(0xC012, 0xAD371282903EB2E4); // -7.094891568758439227560184618114707107977E5L
const LG_S5: L = L::from_bits(0xC00E, 0xF4AA443706E810E3); // -6.263426646464505837422314539808112478303E4L
const LG_S6: L = L::from_bits(0xC009, 0xD29DA60F5DFA05EF); // -1.684926520999477529949915657519454051529E3L
const LG_R0: L = L::from_bits(0xC017, 0x8FBC72CDBD6AA4C7); // -1.883978160734303518163008696712983134698E7L
const LG_R1: L = L::from_bits(0xC017, 0xD6C87E69FFDB436A); // -2.815206082812062064902202753264922306830E7L
const LG_R2: L = L::from_bits(0xC016, 0xF42D96F3D84BACAE); // -1.600245495251915899081846093343626358398E7L
const LG_R3: L = L::from_bits(0xC015, 0x838BFC9A902FB887); // -4.310526301881305003489257052083370058799E6L
const LG_R4: L = L::from_bits(0xC012, 0x87D5CC4AA7C1EF90); // -5.563807682263923279438235987186184968542E5L
const LG_R5: L = L::from_bits(0xC00D, 0xEC8AB16E428C6E2C); // -3.027734654434169996032905158145259713083E4L
const LG_R6: L = L::from_bits(0xC007, 0xE1198B5AF4A1810D); // -4.501995652861105629217250715790764371267E2L
const LG_W0: L = L::from_bits(0x3FFD, 0xD67F1C864BEB4A69); // 4.189385332046727417803e-1L
const LG_W1: L = L::from_bits(0x3FFB, 0xAAAAAAAAAAAA9FCC); // 8.333333333333331447505E-2L
const LG_W2: L = L::from_bits(0xBFF6, 0xB60B60B603A84D88); // -2.777777777750349603440E-3L
const LG_W3: L = L::from_bits(0x3FF4, 0xD00D009230E5F8F2); // 7.936507795855070755671E-4L
const LG_W4: L = L::from_bits(0xBFF4, 0x9C09844E9FCE8B20); // -5.952345851765688514613E-4L
const LG_W5: L = L::from_bits(0x3FF4, 0xDC88D492AAD13BDC); // 8.412723297322498080632E-4L
const LG_W6: L = L::from_bits(0xBFF5, 0xF6853DA103043D91); // -1.880801938119376907179E-3L
const LG_W7: L = L::from_bits(0x3FF7, 0xA01291C2CC08D984); // 4.885026142432270781165E-3L

/// `sin(pi x)` for `x > 0`, exactly zero at the integers (musl's
/// `lgammal`'s `sin_pi`).
fn sin_pi(x: L) -> L {
    // spurious inexact if odd int
    let mut x = x * ld(0.5);
    x = ld(2.0) * (x - floorl(x)); // x mod 2
    #[allow(clippy::cast_possible_truncation)]
    let n = ((x * ld(4.0)).round_int_toward(3).to_i64_rint() as i32 + 1) / 2;
    x = x - L::from_i64(i64::from(n)) * ld(0.5);
    x = x * LG_PI;
    match n {
        1 => k_cosl(x, ZERO),
        2 => k_sinl(-x, ZERO, false),
        3 => -k_cosl(x, ZERO),
        // 0 and 4
        _ => k_sinl(x, ZERO, false),
    }
}

/// One of the zeros of `log|gamma|` below -2, with the constants
/// [`lgammal_near_zero`] evaluates `lgammal` near it with.
///
/// Below -2 musl's `lgammal` -- the rest of this function -- uses the
/// reflection formula, `log(pi / |x sin(pi x)|) - lgamma(-x)`: two numbers
/// the size of `log(n!)` subtracted to give one near zero wherever |gamma(x)|
/// is near 1, twice in every unit interval. There it was right only
/// absolutely (known-issues.md, D-POSIX-LGAMMA-LOSES-DIGITS-NEAR-NEGATIVE-ROOTS).
/// The table is `posix/tools/oracle/lgammal_zeros.py table 30`'s, from
/// mpmath at 80 digits: the zeros for n = 2..=30, beyond which the
/// reflection loses at most a few bits even beside a pole.
struct LdZero {
    /// The integer nearest the zero: the pole it lies beside.
    pole: i32,
    /// The zero's offset from `pole`, `e0`, as `hi + lo`.
    e0: (L, L),
    /// `sin(pi e0)`, as `hi + lo`.
    sin_e0: (L, L),
    /// `cot(pi e0)`, as `hi + lo`.
    cot_e0: (L, L),
    /// `B(d) = lgamma(1 - x0) - lgamma(1 - x0 - d) = sum c_k d^k`: the first
    /// coefficient, `psi(1 - x0)`, as `hi + lo`...
    c1: (L, L),
    /// ...and the rest, `c_k = (-1)^(k+1) psi^(k-1)(1 - x0) / k!` from
    /// `k = 2`, as many as `|d| <= 1/4` needs for 2^-72.
    c: &'static [L],
}

// Double-long-double arithmetic: a value as `hi + lo`, `lo` at most half an
// ulp of `hi` -- twice the 64-bit significand. Each transformation below is
// exact given the x87 unit's 64-bit precision control and rounding to
// nearest: the state a SlateOS (or Linux) thread starts in, and the one
// every function in this file assumes.

/// `a + b` as `(s, e)` exactly, for `|a| >= |b|` or `a` zero.
fn dd_fast_two_sum(a: L, b: L) -> (L, L) {
    let s = a + b;
    (s, b - (s - a))
}

/// `a + b` as `(s, e)` exactly, either way round (Knuth's TwoSum).
fn dd_two_sum(a: L, b: L) -> (L, L) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// `a` as `hi + lo` of at most 32 significant bits each (Veltkamp's split by
/// 2^32 + 1), so that the product of two halves is exact in 64 bits.
fn dd_split(a: L) -> (L, L) {
    const C: L = L::from_bits(0x401F, 0x8000_0000_8000_0000); // 2^32 + 1
    let g = C * a;
    let hi = g - (g - a);
    (hi, a - hi)
}

/// `a b` as `(p, e)` exactly (Dekker's product).
fn dd_two_prod(a: L, b: L) -> (L, L) {
    let p = a * b;
    let (ah, al) = dd_split(a);
    let (bh, bl) = dd_split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

/// `(ah + al)(bh + bl)`, to about 2^-125 relative.
fn dd_mul(ah: L, al: L, bh: L, bl: L) -> (L, L) {
    let (p, e) = dd_two_prod(ah, bh);
    dd_fast_two_sum(p, e + (ah * bl + al * bh))
}

/// `(ah + al) + (bh + bl)`: the high parts' sum exact, the rest to within
/// the low parts' rounding.
fn dd_add(ah: L, al: L, bh: L, bl: L) -> (L, L) {
    let (s, e) = dd_two_sum(ah, bh);
    dd_fast_two_sum(s, e + (al + bl))
}

/// `(ah + al) / (bh + bl)`: a quotient and one correction.
fn dd_div(ah: L, al: L, bh: L, bl: L) -> (L, L) {
    let q = ah / bh;
    let (p, e) = dd_two_prod(q, bh);
    let r = (((ah - p) - e) + al) - q * bl;
    dd_fast_two_sum(q, r / bh)
}

/// `pi` as `hi + lo`.
const DD_PI: (L, L) = (
    L::from_bits(0x4000, 0xC90F_DAA2_2168_C235),
    L::from_bits(0xBFBE, 0xECE6_75D1_FC8F_8CBB),
);

/// `sin(pi (dh + dl))` as `hi + lo`, for `|dh| <= 1/4`: the argument
/// `t = pi d` as a pair, and the series `t - t^3/3! + ...` with its leading
/// term kept as that pair and the rest -- a tenth of it at most -- summed in
/// long double.
fn dd_sinpi(dh: L, dl: L) -> (L, L) {
    // (-1)^k / (2k+1)!, k = 1..=10; the next term is below 2^-70 of t for
    // |t| <= pi/4.
    const S: [L; 10] = [
        L::from_bits(0xBFFC, 0xAAAA_AAAA_AAAA_AAAB),
        L::from_bits(0x3FF8, 0x8888_8888_8888_8889),
        L::from_bits(0xBFF2, 0xD00D_00D0_0D00_D00D),
        L::from_bits(0x3FEC, 0xB8EF_1D2A_B639_9C7D),
        L::from_bits(0xBFE5, 0xD732_2B3F_AA27_1C7F),
        L::from_bits(0x3FDE, 0xB092_309D_4368_4BE5),
        L::from_bits(0xBFD6, 0xD73F_9F39_9DC0_F88F),
        L::from_bits(0x3FCE, 0xCA96_3B81_856A_5359),
        L::from_bits(0xBFC6, 0x97A4_DA34_0A0A_B926),
        L::from_bits(0x3FBD, 0xB8DC_77B6_E7AB_8C5F),
    ];
    let (th, tl) = dd_mul(DD_PI.0, DD_PI.1, dh, dl);
    let t2 = th * th;
    // t^3 (S1 + t^2 S2 + ...), and the first-order effect of `tl` on the
    // cubic term, -t^2 tl / 2.
    let tail = th * t2 * horner_l(t2, &S) - t2 * tl * ld(0.5);
    dd_fast_two_sum(th, tl + tail)
}

/// `1 / (2k + 1)`, `k = 1..=24`: the odd series of `atanh`, `w + w^3/3 + ...`
/// after its first term. For `|w| < 1/3` the next term, `w^51 / 51`, is below
/// 2^-80 of `w`.
const DD_ATANH: [L; 24] = [
    L::from_bits(0x3FFD, 0xAAAA_AAAA_AAAA_AAAB),
    L::from_bits(0x3FFC, 0xCCCC_CCCC_CCCC_CCCD),
    L::from_bits(0x3FFC, 0x9249_2492_4924_9249),
    L::from_bits(0x3FFB, 0xE38E_38E3_8E38_E38E),
    L::from_bits(0x3FFB, 0xBA2E_8BA2_E8BA_2E8C),
    L::from_bits(0x3FFB, 0x9D89_D89D_89D8_9D8A),
    L::from_bits(0x3FFB, 0x8888_8888_8888_8889),
    L::from_bits(0x3FFA, 0xF0F0_F0F0_F0F0_F0F1),
    L::from_bits(0x3FFA, 0xD794_35E5_0D79_435E),
    L::from_bits(0x3FFA, 0xC30C_30C3_0C30_C30C),
    L::from_bits(0x3FFA, 0xB216_42C8_590B_2164),
    L::from_bits(0x3FFA, 0xA3D7_0A3D_70A3_D70A),
    L::from_bits(0x3FFA, 0x97B4_25ED_097B_425F),
    L::from_bits(0x3FFA, 0x8D3D_CB08_D3DC_B08D),
    L::from_bits(0x3FFA, 0x8421_0842_1084_2108),
    L::from_bits(0x3FF9, 0xF83E_0F83_E0F8_3E10),
    L::from_bits(0x3FF9, 0xEA0E_A0EA_0EA0_EA0F),
    L::from_bits(0x3FF9, 0xDD67_C8A6_0DD6_7C8A),
    L::from_bits(0x3FF9, 0xD20D_20D2_0D20_D20D),
    L::from_bits(0x3FF9, 0xC7CE_0C7C_E0C7_CE0C),
    L::from_bits(0x3FF9, 0xBE82_FA0B_E82F_A0BF),
    L::from_bits(0x3FF9, 0xB60B_60B6_0B60_B60B),
    L::from_bits(0x3FF9, 0xAE4C_415C_9882_B931),
    L::from_bits(0x3FF9, 0xA72F_0539_7829_CBC1),
];

/// `log1p(uh + ul)` as `hi + lo`, for `|u| < 1/2`: `2 atanh(w)` with
/// `w = u / (2 + u)`, below 1/3 in magnitude, the leading `2 w` kept as a
/// pair and the odd series after it -- under a twentieth of it -- in long
/// double.
fn dd_log1p(uh: L, ul: L) -> (L, L) {
    let (vh, vl) = dd_add(ld(2.0), ZERO, uh, ul);
    let (wh, wl) = dd_div(uh, ul, vh, vl);
    let w2 = wh * wh;
    let tail = wh * w2 * horner_l(w2, &DD_ATANH);
    let (sh, sl) = dd_fast_two_sum(wh, wl + tail);
    (ld(2.0) * sh, ld(2.0) * sl)
}

/// `log 2` as `hi + lo`.
const DD_LN2: (L, L) = (
    L::from_bits(0x3FFE, 0xB172_17F7_D1CF_79AC),
    L::from_bits(0xBFBC, 0xD871_319F_F034_2543),
);

/// `log(rh + rl)` as `hi + lo`, for a positive finite normal `rh`: `r` scaled
/// by a power of two, exactly, into `[sqrt(1/2), sqrt(2))`, where
/// `w = (m - 1) / (m + 1)` is at most 0.172 and `2 atanh(w)` needs 14 terms;
/// then `k log 2` added.
fn dd_log(rh: L, rl: L) -> (L, L) {
    const SQRT2: L = L::from_bits(0x3FFF, 0xB504_F333_F9DE_6484);
    let mut k = i32::from(rh.biased_exponent()) - 0x3FFF;
    let (mut mh, mut ml) = (rh.scalbn(-k), rl.scalbn(-k));
    if mh >= SQRT2 {
        k += 1;
        (mh, ml) = (mh * ld(0.5), ml * ld(0.5));
    }
    let (nh, nl) = dd_add(mh, ml, -ONE, ZERO);
    let (dh, dl) = dd_add(mh, ml, ONE, ZERO);
    let (wh, wl) = dd_div(nh, nl, dh, dl);
    let w2 = wh * wh;
    let tail = wh * w2 * horner_l(w2, &DD_ATANH);
    let (sh, sl) = dd_fast_two_sum(wh, wl + tail);
    let kl = L::from_i64(i64::from(k));
    let (kh, ke) = dd_two_prod(kl, DD_LN2.0);
    dd_add(kh, ke + kl * DD_LN2.1, ld(2.0) * sh, ld(2.0) * sl)
}

/// `lgammal(x)` for `x` below -2 and within 1/4 of a zero of `lgamma`, where
/// the reflection formula cancels; `None` elsewhere, where it does not.
///
/// With `x0` the zero and `d = x - x0`, the reflection formula at `x` less
/// the same at `x0`, where `lgamma` is 0, is
///
/// ```text
/// lgamma(x) = A + B,   A = log|sin(pi x0) / sin(pi x)|,
///                      B = lgamma(1 - x0) - lgamma(1 - x),
/// ```
///
/// both small wherever the result is: nothing the size of `log(n!)` is
/// subtracted. They can still cancel each other by a factor of a few --
/// by 2.2 at the zero near -2.748, where `A` falls three times as fast as
/// `B` rises -- so both are formed in double-long-double: `A` as
/// `-log1p(u)`, `u = sin(pi x) / sin(pi x0) - 1` formed without subtracting
/// the sines, `cot(pi e0) sin(pi d) - 2 sin(pi d / 2)^2` with `e0` the
/// zero's offset from its pole; `B` as its Taylor series about `1 - x0`,
/// which is above 3, so it converges fast, the leading `psi(1 - x0) d`
/// exact. Where `|u| >= 1/2`, `A` is the logarithm of the ratio itself,
/// still in pairs: `1 + u` above 3/2, and below 1/2 the quotient of the two
/// sines, which beside a pole keeps the digits `1 + u` would lose.
fn lgammal_near_zero(x: L) -> Option<L> {
    let n = (-x).round_int_toward(3).to_i64_rint();
    if !(2..=30).contains(&n) {
        return None;
    }
    // x is in (-n - 1, -n), whose zeros are these two.
    let base = usize::try_from((n - 2) * 2).ok()?;
    let mut best: Option<(&LdZero, L, L, L)> = None;
    for z in LGAMMAL_ZEROS.get(base..base.checked_add(2)?)? {
        // Exact: x is within 1 of the pole and at least 2 in magnitude.
        let e = x - L::from_i64(i64::from(z.pole));
        // d = e - e0 as a pair: the first difference exact, the second
        // rounded where it is already far below the first.
        let (t, te) = dd_two_sum(e, -z.e0.0);
        let (dh, dl) = dd_fast_two_sum(t, te - z.e0.1);
        if best.is_none_or(|(_, _, bh, _)| dh.abs() < bh.abs()) {
            best = Some((z, e, dh, dl));
        }
    }
    let (z, e, dh, dl) = best?;
    let half = ld(0.5);
    if dh.abs() > half * half {
        return None;
    }
    // u = cot(pi e0) sin(pi d) - 2 sin(pi d / 2)^2
    let (sh, sl) = dd_sinpi(dh, dl);
    let (qh, ql) = dd_sinpi(dh * half, dl * half);
    let (ksh, ksl) = dd_mul(z.cot_e0.0, z.cot_e0.1, sh, sl);
    let (q2h, q2l) = dd_mul(qh, ql, qh, ql);
    let (uh, ul) = dd_add(ksh, ksl, ld(-2.0) * q2h, ld(-2.0) * q2l);
    let (ah, al) = if uh.abs() < half {
        let (lh, ll) = dd_log1p(uh, ul);
        (-lh, -ll)
    } else {
        // The ratio sin(pi x) / sin(pi x0), positive: x and the zero share an
        // interval. Above 3/2 it is 1 + u, which loses nothing; below 1/2 --
        // x nearer the pole than the zero is, |e| < |e0| <= 1/4 -- it is the
        // quotient of the two sines, since beside the pole it is far smaller
        // than u's rounding.
        let (rh, rl) = if uh > ZERO {
            dd_add(ONE, ZERO, uh, ul)
        } else {
            let (seh, sel) = dd_sinpi(e, ZERO);
            dd_div(seh, sel, z.sin_e0.0, z.sin_e0.1)
        };
        if rh <= ZERO {
            return None;
        }
        let (lh, ll) = dd_log(rh, rl);
        (-lh, -ll)
    };
    // B = d (c1 + d (c2 + ...)): c1 d as a pair, the rest -- a few hundredths
    // of it at most -- in long double.
    let rest = dh * dh * horner_l(dh, z.c);
    let (bh, bl) = dd_mul(z.c1.0, z.c1.1, dh, dl);
    let (bh, bl) = dd_fast_two_sum(bh, bl + rest);
    let (rh, rl) = dd_add(ah, al, bh, bl);
    Some(rh + rl)
}

// 58 zeros, n = 2..=30; at most 18 coefficients each.
#[rustfmt::skip]
static LGAMMAL_ZEROS: [LdZero; 58] = [
    LdZero {
        // -2.457024738220800623039454
        pole: -2,
        e0: (L::from_bits(0xBFFD, 0xE9FF25803E1B0558), L::from_bits(0x3FBB, 0x9B0675072FC769E6)),
        sin_e0: (L::from_bits(0xBFFE, 0xFDAB9D5B090D8C8A), L::from_bits(0xBFBD, 0xD8497535758B32A1)),
        cot_e0: (L::from_bits(0xBFFC, 0x8B18E25FF6D1C846), L::from_bits(0x3FBB, 0x8DA7C4E6343744EF)),
        c1: (L::from_bits(0x3FFF, 0x8B5FB7B6880FD368), L::from_bits(0xBFBE, 0xDF2BBC1C473B8BCB)),
        c: &[
            L::from_bits(0xBFFC, 0xAB8EC74C2879E6D6),
            L::from_bits(0xBFF9, 0x97F27937C683E28B),
            L::from_bits(0xBFF6, 0xC82F93C76A0F320A),
            L::from_bits(0xBFF4, 0x9CFFA779944CAF99),
            L::from_bits(0xBFF2, 0x87CC4E40B2CF4B51),
            L::from_bits(0xBFEF, 0xF9F57F5313486EB1),
            L::from_bits(0xBFED, 0xEFFD01F376FD83A3),
            L::from_bits(0xBFEB, 0xED83885F6FA57F5A),
            L::from_bits(0xBFE9, 0xF06EB7BFADB05DAD),
            L::from_bits(0xBFE7, 0xF7A1CEC1A5CB6A26),
            L::from_bits(0xBFE6, 0x8143907B7350BE06),
            L::from_bits(0xBFE4, 0x886BFBE015BDDB73),
            L::from_bits(0xBFE2, 0x91400932001E660F),
            L::from_bits(0xBFE0, 0x9BC73EE73897AF09),
            L::from_bits(0xBFDE, 0xA816AE7BE6778C48),
            L::from_bits(0xBFDC, 0xB64EC6D4473CA1D7),
            L::from_bits(0xBFDA, 0xC69A4C5DF70F590E),
        ],
    },
    LdZero {
        // -2.747682646727412601391488
        pole: -3,
        e0: (L::from_bits(0x3FFD, 0x812FBD7909BFCD0D), L::from_bits(0xBFBB, 0xAD25A320F575FA58)),
        sin_e0: (L::from_bits(0x3FFE, 0xB65516E54FC82724), L::from_bits(0x3FBC, 0xEFF1B7A299A02063)),
        cot_e0: (L::from_bits(0x3FFE, 0xFC4CA701ABE590FD), L::from_bits(0x3FBC, 0xB9C924C144432D52)),
        c1: (L::from_bits(0x3FFF, 0x974630E62BE0B753), L::from_bits(0x3FBE, 0xCEFE9899DC066DF9)),
        c: &[
            L::from_bits(0xBFFC, 0x9C71A2840322D3AF),
            L::from_bits(0xBFF8, 0xFD10E9BFD589FCDA),
            L::from_bits(0xBFF6, 0x986DD71D29FE701F),
            L::from_bits(0xBFF3, 0xDADF061EA0EBD0D3),
            L::from_bits(0xBFF1, 0xAD7C9A9CA12F6937),
            L::from_bits(0xBFEF, 0x9273722965972C4A),
            L::from_bits(0xBFED, 0x81139D60C62D2E33),
            L::from_bits(0xBFEA, 0xEAB41BADF71D083A),
            L::from_bits(0xBFE8, 0xDA645728F3603113),
            L::from_bits(0xBFE6, 0xCEDE94FFDB143A14),
            L::from_bits(0xBFE4, 0xC6B89CE10D65836B),
            L::from_bits(0xBFE2, 0xC10CE17FF45D2668),
            L::from_bits(0xBFE0, 0xBD43E72302714D58),
            L::from_bits(0xBFDE, 0xBAF669B7353BC545),
            L::from_bits(0xBFDC, 0xB9DC28EF91AE759A),
            L::from_bits(0xBFDA, 0xB9C18BF7DB0B9D43),
        ],
    },
    LdZero {
        // -3.143580888349980058694359
        pole: -3,
        e0: (L::from_bits(0xBFFC, 0x9306DE4F2CD7BEE3), L::from_bits(0x3FB7, 0xCB32961322CD5D5A)),
        sin_e0: (L::from_bits(0xBFFD, 0xDF325E7ED8C79966), L::from_bits(0xBFBC, 0x84FBB464DC1B23EF)),
        cot_e0: (L::from_bits(0xC000, 0x8420C5DDE11E850B), L::from_bits(0xBFBF, 0x8F3A103BDD7C91E3)),
        c1: (L::from_bits(0x3FFF, 0xA5E57AF2D3ADC83A), L::from_bits(0x3FBD, 0xACEB7BA6B1193B61)),
        c: &[
            L::from_bits(0xBFFC, 0x8BA9393415BA6568),
            L::from_bits(0xBFF8, 0xC9F93DF17165C7F2),
            L::from_bits(0xBFF5, 0xD9CFFCB0B4535B1D),
            L::from_bits(0xBFF3, 0x8C2964C606CDBFDA),
            L::from_bits(0xBFF0, 0xC7609E395FC293F5),
            L::from_bits(0xBFEE, 0x972C830BF1444169),
            L::from_bits(0xBFEB, 0xEF913D6CE1CEDE96),
            L::from_bits(0xBFE9, 0xC3F797E3FF64DF77),
            L::from_bits(0xBFE7, 0xA42E48B60FC81126),
            L::from_bits(0xBFE5, 0x8C1CBD9CC038C364),
            L::from_bits(0xBFE2, 0xF2A74E062D1A485B),
            L::from_bits(0xBFE0, 0xD4987E7F66A16690),
            L::from_bits(0xBFDE, 0xBC0D49D41D9C1AD1),
            L::from_bits(0xBFDC, 0xA7AA3071854B231A),
            L::from_bits(0xBFDA, 0x967BD863A05F87A2),
            L::from_bits(0xBFD8, 0x87D33F66C0EA4A1A),
        ],
    },
    LdZero {
        // -3.955294284858597928532797
        pole: -4,
        e0: (L::from_bits(0x3FFA, 0xB71D5707A035E8F3), L::from_bits(0xBFB9, 0x8AC252246D986415)),
        sin_e0: (L::from_bits(0x3FFC, 0x8F5874D99AEBF587), L::from_bits(0x3FB9, 0x951A06F7B82F44C7)),
        cot_e0: (L::from_bits(0x4001, 0xE257F8E2C9659C6E), L::from_bits(0x3FBF, 0x9D24FA2942AD0489)),
        c1: (L::from_bits(0x3FFF, 0xBF82A2C85DEF58BC), L::from_bits(0xBFBA, 0xB55A6D65E584056D)),
        c: &[
            L::from_bits(0xBFFB, 0xE4E3EFD2DB0E7255),
            L::from_bits(0xBFF8, 0x87E219E358CE63CD),
            L::from_bits(0xBFF4, 0xF10D5C802D6E86F0),
            L::from_bits(0xBFF1, 0xFF96C8A7AB3B302E),
            L::from_bits(0xBFEF, 0x95FFB4365F4AF641),
            L::from_bits(0xBFEC, 0xBBF63D0DBCB923DA),
            L::from_bits(0xBFE9, 0xF6750FDF5E94D92B),
            L::from_bits(0xBFE7, 0xA7021A54DDA4E1C1),
            L::from_bits(0xBFE4, 0xE811DBFC8978741D),
            L::from_bits(0xBFE2, 0xA46722CEE86753BD),
            L::from_bits(0xBFDF, 0xEC8F148A17C350D6),
            L::from_bits(0xBFDD, 0xAC554CBDC7D56CBE),
            L::from_bits(0xBFDA, 0xFDAE938F9F639688),
            L::from_bits(0xBFD8, 0xBC517C4A29B221A6),
            L::from_bits(0xBFD6, 0x8CCE379943F73AD5),
        ],
    },
    LdZero {
        // -4.039361839740536874234577
        pole: -4,
        e0: (L::from_bits(0xBFFA, 0xA139E16656030C3A), L::from_bits(0x3FB6, 0xF4F21E7EED53E840)),
        sin_e0: (L::from_bits(0xBFFB, 0xFC9BC1069587C199), L::from_bits(0xBFBA, 0xBA25C6F24F13C369)),
        cot_e0: (L::from_bits(0xC002, 0x80BA6004BDF7DF78), L::from_bits(0xBFC1, 0x987B096B1511FB30)),
        c1: (L::from_bits(0x3FFF, 0xC1E4B2564D190A3F), L::from_bits(0x3FBE, 0x82AE923879959B65)),
        c: &[
            L::from_bits(0xBFFB, 0xE0AF5D25FA6CEA26),
            L::from_bits(0xBFF8, 0x82F46B021320EC55),
            L::from_bits(0xBFF4, 0xE41AE3EECD85D83E),
            L::from_bits(0xBFF1, 0xED83C159FE2C43ED),
            L::from_bits(0xBFEF, 0x88E735E1B1D198DA),
            L::from_bits(0xBFEC, 0xA88231F6595B8DEB),
            L::from_bits(0xBFE9, 0xD90DA73E9D2973FF),
            L::from_bits(0xBFE7, 0x9080D67B100AC75C),
            L::from_bits(0xBFE4, 0xC54B1BBD9C97B298),
            L::from_bits(0xBFE2, 0x8956BE0F0A217E61),
            L::from_bits(0xBFDF, 0xC232D408663166DF),
            L::from_bits(0xBFDD, 0x8B09494C81296E42),
            L::from_bits(0xBFDA, 0xC9275E23F68FA90C),
            L::from_bits(0xBFD8, 0x92C4ED0F0003CF58),
            L::from_bits(0xBFD5, 0xD7BBD68BC77EC6D2),
        ],
    },
    LdZero {
        // -4.99154464056004772234526
        pole: -5,
        e0: (L::from_bits(0x3FF8, 0x8A8859115032BBCC), L::from_bits(0xBFB6, 0xAA4076988501D7D8)),
        sin_e0: (L::from_bits(0x3FF9, 0xD994B7676B00F839), L::from_bits(0x3FB8, 0xC7709178D2525854)),
        cot_e0: (L::from_bits(0x4004, 0x968C5DF154631C93), L::from_bits(0xBFC3, 0x8733F42B04FABBC0)),
        c1: (L::from_bits(0x3FFF, 0xDA2FC97AA6F6C6BD), L::from_bits(0x3FBD, 0xED6A13B80CB0F0FF)),
        c: &[
            L::from_bits(0xBFFB, 0xB9F583DB79B1B91D),
            L::from_bits(0xBFF7, 0xB39F82962FEB6A6F),
            L::from_bits(0xBFF4, 0x81C86187E648FEE2),
            L::from_bits(0xBFF0, 0xE077C4F086A84614),
            L::from_bits(0xBFED, 0xD7230AC29932688B),
            L::from_bits(0xBFEA, 0xDC5FE926F33F39FF),
            L::from_bits(0xBFE7, 0xEC748AE836CD3637),
            L::from_bits(0xBFE5, 0x833E9FF6DC5C42FA),
            L::from_bits(0xBFE2, 0x9584553898175E8C),
            L::from_bits(0xBFDF, 0xADD22F29EB2F206F),
            L::from_bits(0xBFDC, 0xCD61540EFBD4BA80),
            L::from_bits(0xBFD9, 0xF5E597AD7FF9B94A),
            L::from_bits(0xBFD7, 0x94D1E8CFACB449F4),
            L::from_bits(0xBFD4, 0xB5CA48A9295B0520),
        ],
    },
    LdZero {
        // -5.008218168322593521552368
        pole: -5,
        e0: (L::from_bits(0xBFF8, 0x86A57F0B6D90CA93), L::from_bits(0xBFB5, 0xADCB2A729BD98D5E)),
        sin_e0: (L::from_bits(0xBFF9, 0xD37A8B073D058242), L::from_bits(0xBFB7, 0x9F8E6C0278AD7EC0)),
        cot_e0: (L::from_bits(0xC004, 0x9AE53A33780D7C78), L::from_bits(0x3FC3, 0x88244C2C223F95F8)),
        c1: (L::from_bits(0x3FFF, 0xDA92DB43CDE80671), L::from_bits(0x3FBD, 0xD27A00AB001972DF)),
        c: &[
            L::from_bits(0xBFFB, 0xB96630699CDC6C07),
            L::from_bits(0xBFF7, 0xB28BC4F698F4D75D),
            L::from_bits(0xBFF4, 0x809EC0B074B40ED2),
            L::from_bits(0xBFF0, 0xDDCC29E47B747D29),
            L::from_bits(0xBFED, 0xD3F3486F8994A0F7),
            L::from_bits(0xBFEA, 0xD8790B9A268998C0),
            L::from_bits(0xBFE7, 0xE796E854154FFD69),
            L::from_bits(0xBFE5, 0x802B6676F8F6358D),
            L::from_bits(0xBFE2, 0x919713396284E303),
            L::from_bits(0xBFDF, 0xA8C44A30B5D9A4C9),
            L::from_bits(0xBFDC, 0xC6D57B5007E41014),
            L::from_bits(0xBFD9, 0xED602D77B814AA71),
            L::from_bits(0xBFD7, 0x8F406BA70F15DC5C),
            L::from_bits(0xBFD4, 0xAE7D1F6349C8113D),
        ],
    },
    LdZero {
        // -5.998607480080875629442408
        pole: -6,
        e0: (L::from_bits(0x3FF5, 0xB6853705F9504562), L::from_bits(0x3FB4, 0xA74D0B38481C701C)),
        sin_e0: (L::from_bits(0x3FF7, 0x8F59C7EB986AE18A), L::from_bits(0xBFB6, 0xCCFE120375FD3590)),
        cot_e0: (L::from_bits(0x4006, 0xE49584E65D899479), L::from_bits(0x3FC2, 0xFFAE86F1A539D531)),
        c1: (L::from_bits(0x3FFF, 0xEFB063DB3E08815F), L::from_bits(0x3FBD, 0xCF00554D93F8B9B6)),
        c: &[
            L::from_bits(0xBFFB, 0x9D4389DDE6A5728C),
            L::from_bits(0xBFF7, 0x80900112221E5F2C),
            L::from_bits(0xBFF3, 0x9D58EDBCBEEF5911),
            L::from_bits(0xBFEF, 0xE6A821051320450B),
            L::from_bits(0xBFEC, 0xBB7F2FD8BFD598E4),
            L::from_bits(0xBFE9, 0xA3008487267C325C),
            L::from_bits(0xBFE6, 0x9486E2B971022891),
            L::from_bits(0xBFE3, 0x8C1ABA8DDB3BBFC9),
            L::from_bits(0xBFE0, 0x87B4104E2BA9D91F),
            L::from_bits(0xBFDD, 0x86343160EAF727FE),
            L::from_bits(0xBFDA, 0x86F5936AC9DEAAD1),
            L::from_bits(0xBFD7, 0x8996D656F55AF461),
            L::from_bits(0xBFD4, 0x8DDFB870B6C97AC5),
        ],
    },
    LdZero {
        // -6.001385294453155097261982
        pole: -6,
        e0: (L::from_bits(0xBFF5, 0xB592C4BE4676C0F8), L::from_bits(0xBFB4, 0xB65B458E172E1AB1)),
        sin_e0: (L::from_bits(0xBFF7, 0x8E9B5DA457B87C64), L::from_bits(0x3FB6, 0xDF7A0CF13F4F0534)),
        cot_e0: (L::from_bits(0xC006, 0xE5C6BD9E0A007612), L::from_bits(0x3FC5, 0x9E81DB479E20ACAC)),
        c1: (L::from_bits(0x3FFF, 0xEFBE5DC47E4C157C), L::from_bits(0xBFBD, 0xEA20E5F2DC995BA8)),
        c: &[
            L::from_bits(0xBFFB, 0x9D326767E7D74574),
            L::from_bits(0xBFF7, 0x80740C79EAF052B8),
            L::from_bits(0xBFF3, 0x9D25B6DC83139B5B),
            L::from_bits(0xBFEF, 0xE6443C23F11A1747),
            L::from_bits(0xBFEC, 0xBB19E3EBE7B29367),
            L::from_bits(0xBFE9, 0xA2970D7BCD76C2D4),
            L::from_bits(0xBFE6, 0x9416FD27709B18C1),
            L::from_bits(0xBFE3, 0x8BA25454E162D0FB),
            L::from_bits(0xBFE0, 0x87311D69E5DE986B),
            L::from_bits(0xBFDD, 0x85A490AB2C04A438),
            L::from_bits(0xBFDA, 0x8656FAC3692C1322),
            L::from_bits(0xBFD7, 0x88E6C22B4D42DCF3),
            L::from_bits(0xBFD4, 0x8D1B5C87FEBC5AC8),
        ],
    },
    LdZero {
        // -6.999801507890637697892097
        pole: -7,
        e0: (L::from_bits(0x3FF2, 0xD02251E4400C29D9), L::from_bits(0x3FB1, 0x9347B805BC17F9FF)),
        sin_e0: (L::from_bits(0x3FF4, 0xA377D55E4F8CE61D), L::from_bits(0xBFB3, 0xA2EA64E1025C4EE0)),
        cot_e0: (L::from_bits(0x4009, 0xC874792C955F8726), L::from_bits(0x3FC8, 0xB5C859E8893378F5)),
        c1: (L::from_bits(0x4000, 0x80FFD6454CB8E63D), L::from_bits(0x3FBE, 0x8786DBE61E309A32)),
        c: &[
            L::from_bits(0xBFFB, 0x8855FD96533840CD),
            L::from_bits(0xBFF6, 0xC15630DD6C4CC728),
            L::from_bits(0xBFF2, 0xCD5441AB37AC5707),
            L::from_bits(0xBFEF, 0x82A6E8325F362917),
            L::from_bits(0xBFEB, 0xB87B73C95610A00A),
            L::from_bits(0xBFE8, 0x8B5AB606C62F1075),
            L::from_bits(0xBFE4, 0xDCC1193590D1178E),
            L::from_bits(0xBFE1, 0xB515010E02CAAB65),
            L::from_bits(0xBFDE, 0x9894AF6607BD2236),
            L::from_bits(0xBFDB, 0x8351007E9555D601),
            L::from_bits(0xBFD7, 0xE5EC67E2B82BD762),
            L::from_bits(0xBFD4, 0xCC213381C26E0D91),
            L::from_bits(0xBFD1, 0xB75BFFE47BFA90F1),
        ],
    },
    LdZero {
        // -7.000198333407324751606981
        pole: -7,
        e0: (L::from_bits(0xBFF2, 0xCFF7B7F87ADF4483), L::from_bits(0x3FB0, 0x8C919E1F5536678D)),
        sin_e0: (L::from_bits(0xBFF4, 0xA3565FE137EC26DE), L::from_bits(0xBFB2, 0xC9B7A7F9CCDE8B50)),
        cot_e0: (L::from_bits(0xC009, 0xC89D89212ACC99A9), L::from_bits(0xBFC8, 0xCAB8D5A530064664)),
        c1: (L::from_bits(0x4000, 0x8100B3DD67B750A3), L::from_bits(0x3FBE, 0x8C48D2AECA6D95A3)),
        c: &[
            L::from_bits(0xBFFB, 0x8854263D181C3331),
            L::from_bits(0xBFF6, 0xC150FA012E88124E),
            L::from_bits(0xBFF2, 0xCD4BF64732F304FB),
            L::from_bits(0xBFEF, 0x829FE14A85A0E09F),
            L::from_bits(0xBFEB, 0xB86F1132C8D7E020),
            L::from_bits(0xBFE8, 0x8B4F800B65CCAAC3),
            L::from_bits(0xBFE4, 0xDCAC685F0DAC5753),
            L::from_bits(0xBFE1, 0xB501A221717167CB),
            L::from_bits(0xBFDE, 0x988258E9845EA4B4),
            L::from_bits(0xBFDB, 0x833F7D2C9976D4DA),
            L::from_bits(0xBFD7, 0xE5CAB7FCC5236220),
            L::from_bits(0xBFD4, 0xCC009D6E5C76751A),
            L::from_bits(0xBFD1, 0xB73C54100D12364C),
        ],
    },
    LdZero {
        // -7.999975197095820664154336
        pole: -8,
        e0: (L::from_bits(0x3FEF, 0xD00FD4C61E1AB0BF), L::from_bits(0xBFAE, 0xB522B0CA9D2E8838)),
        sin_e0: (L::from_bits(0x3FF1, 0xA36950AB7F61BF16), L::from_bits(0xBFAF, 0xE41A12FCD51FFFFB)),
        cot_e0: (L::from_bits(0x400C, 0xC8864AEC4A2F1C21), L::from_bits(0x3FCB, 0x8551CA48F8866A41)),
        c1: (L::from_bits(0x4000, 0x890038E37ED94267), L::from_bits(0x3FBF, 0xA635D83D5244754C)),
        c: &[
            L::from_bits(0xBFFA, 0xF0AA518AFA62B913),
            L::from_bits(0xBFF6, 0x96A923E68A0943C4),
            L::from_bits(0xBFF2, 0x8D506C3270351FA0),
            L::from_bits(0xBFEE, 0x9EE0DB1872C39A91),
            L::from_bits(0xBFEA, 0xC6409515D277965F),
            L::from_bits(0xBFE7, 0x8461A6CB9D07A923),
            L::from_bits(0xBFE3, 0xB96E74ED72BC45AF),
            L::from_bits(0xBFE0, 0x868933821C5E41BC),
            L::from_bits(0xBFDC, 0xC895C77C01145774),
            L::from_bits(0xBFD9, 0x98C4FFD7733CC232),
            L::from_bits(0xBFD5, 0xECC57717F85AD0CA),
            L::from_bits(0xBFD2, 0xBA1DDB118E52A131),
        ],
    },
    LdZero {
        // -8.00002480027068195969771
        pole: -8,
        e0: (L::from_bits(0xBFEF, 0xD00A2CFE4FB0659E), L::from_bits(0xBFAE, 0xEC1CEC8576677CA5)),
        sin_e0: (L::from_bits(0xBFF1, 0xA364DF95F5600B41), L::from_bits(0x3FB0, 0xF6EE303B25E2AC20)),
        cot_e0: (L::from_bits(0xC00C, 0xC88BBE678014C75E), L::from_bits(0x3FCB, 0xEB6249C0FDEAD2B2)),
        c1: (L::from_bits(0x4000, 0x890051564DA6A19E), L::from_bits(0xBFBE, 0x833F9FC3DD635828)),
        c: &[
            L::from_bits(0xBFFA, 0xF0A9F5B64E2D6113),
            L::from_bits(0xBFF6, 0x96A8B10E46238262),
            L::from_bits(0xBFF2, 0x8D4FCACC75FC8771),
            L::from_bits(0xBFEE, 0x9EDFE96B87A299E7),
            L::from_bits(0xBFEA, 0xC63F1C8A43D6CD3E),
            L::from_bits(0xBFE7, 0x84607966D8613411),
            L::from_bits(0xBFE3, 0xB96C88EBF72A567E),
            L::from_bits(0xBFE0, 0x86879BFB1C19D119),
            L::from_bits(0xBFDC, 0xC8931CA605FE5A10),
            L::from_bits(0xBFD9, 0x98C2BE96BAE9ECD4),
            L::from_bits(0xBFD5, 0xECC19FF588EF5B90),
            L::from_bits(0xBFD2, 0xBA1A90D864F90826),
        ],
    },
    LdZero {
        // -8.999997244250977468194357
        pole: -9,
        e0: (L::from_bits(0x3FEC, 0xB8EF685FC00DE6C6), L::from_bits(0xBFAB, 0xABA492CF3F101087)),
        sin_e0: (L::from_bits(0x3FEE, 0x913F6CEB42205201), L::from_bits(0xBFAD, 0xCDB8ACC350D86D41)),
        cot_e0: (L::from_bits(0x400F, 0xE199C990F95BF448), L::from_bits(0xBFCE, 0xFFADAC8C98AF64C5)),
        c1: (L::from_bits(0x4000, 0x901CB5ACFF755874), L::from_bits(0xBFBF, 0xF6A34D8CDA821D56)),
        c: &[
            L::from_bits(0xBFFA, 0xD76176B96B003937),
            L::from_bits(0xBFF5, 0xF163311837F8FD97),
            L::from_bits(0xBFF1, 0xCAB75BD40C56712A),
            L::from_bits(0xBFED, 0xCC1A9190E3DFC46F),
            L::from_bits(0xBFE9, 0xE4212BEE857E92DE),
            L::from_bits(0xBFE6, 0x887A1AE51AEB56C4),
            L::from_bits(0xBFE2, 0xAB4E82644E1B4982),
            L::from_bits(0xBFDE, 0xDECB8464E5231E5E),
            L::from_bits(0xBFDB, 0x94E4006BF5AF4872),
            L::from_bits(0xBFD7, 0xCB5A52B82E74AE39),
            L::from_bits(0xBFD4, 0x8D5248179612A1C6),
            L::from_bits(0xBFD0, 0xC7481EB0FA616D6C),
        ],
    },
    LdZero {
        // -9.000002755714822650346361
        pole: -9,
        e0: (L::from_bits(0xBFEC, 0xB8EED1F61B5B6110), L::from_bits(0x3FA7, 0x91E5710A4297B09D)),
        sin_e0: (L::from_bits(0xBFEE, 0x913EF6C8FF2A5AD7), L::from_bits(0xBFAD, 0xAA6C7CC5DF7442AD)),
        cot_e0: (L::from_bits(0xC00F, 0xE19A810E3CAA55D3), L::from_bits(0xBFCE, 0x811CBA5773BF834C)),
        c1: (L::from_bits(0x4000, 0x901CB81B5C50FE3B), L::from_bits(0x3FBF, 0xEBE29806C16CE4DC)),
        c: &[
            L::from_bits(0xBFFA, 0xD7616E8CE232D54D),
            L::from_bits(0xBFF5, 0xF1631ECA14BA4943),
            L::from_bits(0xBFF1, 0xCAB744CA48997F9F),
            L::from_bits(0xBFED, 0xCC1A72AA7A71D238),
            L::from_bits(0xBFE9, 0xE42100CC5FBFED7C),
            L::from_bits(0xBFE6, 0x8879FBF5031FA295),
            L::from_bits(0xBFE2, 0xAB4E552029C84A4D),
            L::from_bits(0xBFDE, 0xDECB412B9934F7AB),
            L::from_bits(0xBFDB, 0x94E3CDECA395841E),
            L::from_bits(0xBFD7, 0xCB5A0626E37F59D3),
            L::from_bits(0xBFD4, 0x8D520D9BB6118CA0),
            L::from_bits(0xBFD0, 0xC747C4CC1D7E0DCC),
        ],
    },
    LdZero {
        // -9.999999724426629166468352
        pole: -10,
        e0: (L::from_bits(0x3FE9, 0x93F2840465F3AA31), L::from_bits(0x3FA5, 0xB5F57072C5A2921A)),
        sin_e0: (L::from_bits(0x3FEA, 0xE865266ECEE0D74A), L::from_bits(0xBFA9, 0x83BE0AB94F12DE81)),
        cot_e0: (L::from_bits(0x4013, 0x8D005154C7EF2C88), L::from_bits(0x3FD0, 0xD123E36F205B3BC4)),
        c1: (L::from_bits(0x4000, 0x96831D2E6C090F90), L::from_bits(0xBFBE, 0x904ED8F787E7CE55)),
        c: &[
            L::from_bits(0xBFFA, 0xC2E691B1274CDC89),
            L::from_bits(0xBFF5, 0xC5B25916FDCC7269),
            L::from_bits(0xBFF1, 0x96498B3EDAD1BE53),
            L::from_bits(0xBFED, 0x88FEA48B33779DDB),
            L::from_bits(0xBFE9, 0x8AA699898309F719),
            L::from_bits(0xBFE5, 0x963D7A33D7E4AC62),
            L::from_bits(0xBFE1, 0xAAD0627E40843BED),
            L::from_bits(0xBFDD, 0xC940C9A3C8E24B60),
            L::from_bits(0xBFD9, 0xF3B7A2582E4007D2),
            L::from_bits(0xBFD6, 0x96D1741AB7A3F9DD),
            L::from_bits(0xBFD2, 0xBDFBE432BC21E4E9),
        ],
    },
    LdZero {
        // -10.00000027557301364660025
        pole: -10,
        e0: (L::from_bits(0xBFE9, 0x93F2777324F68BB3), L::from_bits(0x3FA8, 0x87AAEF87F3DE491C)),
        sin_e0: (L::from_bits(0xBFEA, 0xE86512B1285671F9), L::from_bits(0x3FA7, 0xF6D70D7F44131161)),
        cot_e0: (L::from_bits(0xC013, 0x8D005D4EFD5A00D4), L::from_bits(0x3FD2, 0xB671255414A46AB2)),
        c1: (L::from_bits(0x4000, 0x96831D66BD8AA2CC), L::from_bits(0xBFBF, 0xE09FC751513EE9FE)),
        c: &[
            L::from_bits(0xBFFA, 0xC2E69105C64A2CDB),
            L::from_bits(0xBFF5, 0xC5B257BB9375107F),
            L::from_bits(0xBFF1, 0x964989B2FEE60775),
            L::from_bits(0xBFED, 0x88FEA2AA6D7D3AB9),
            L::from_bits(0xBFE9, 0x8AA69729B9C4ABF7),
            L::from_bits(0xBFE5, 0x963D771E1C429B0A),
            L::from_bits(0xBFE1, 0xAAD05E677B0DABB1),
            L::from_bits(0xBFDD, 0xC940C4234BF0CCFD),
            L::from_bits(0xBFD9, 0xF3B79ADAA57A6BC8),
            L::from_bits(0xBFD6, 0x96D16EF52C8B25AC),
            L::from_bits(0xBFD2, 0xBDFBDD1254483D55),
        ],
    },
    LdZero {
        // -10.99999997494789008152378
        pole: -11,
        e0: (L::from_bits(0x3FE5, 0xD7322C1C9924A65B), L::from_bits(0xBFA4, 0xB6F4634169391602)),
        sin_e0: (L::from_bits(0x3FE7, 0xA903B85C0D505A7A), L::from_bits(0xBFA3, 0x918B471E7E60D35F)),
        cot_e0: (L::from_bits(0x4016, 0xC1E077498C57C270), L::from_bits(0x3FD3, 0xCB927391BF01F0F4)),
        c1: (L::from_bits(0x4000, 0x9C5491A555A2D56F), L::from_bits(0xBFBF, 0x93C43B23C16DF19A)),
        c: &[
            L::from_bits(0xBFFA, 0xB1F99BF60F46A8C7),
            L::from_bits(0xBFF5, 0xA4DF08202BE50D66),
            L::from_bits(0xBFF0, 0xE4F49451210A7FCF),
            L::from_bits(0xBFEC, 0xBEA69599D5EAD1CF),
            L::from_bits(0xBFE8, 0xB048F48FF6EF51BB),
            L::from_bits(0xBFE4, 0xAE899C5C974B584A),
            L::from_bits(0xBFE0, 0xB55654C2634C1B32),
            L::from_bits(0xBFDC, 0xC342F26FAF29D027),
            L::from_bits(0xBFD8, 0xD821EF803D89DFBC),
            L::from_bits(0xBFD4, 0xF486B1E0B1356546),
            L::from_bits(0xBFD1, 0x8CCDB3C2AF548C28),
        ],
    },
    LdZero {
        // -11.00000002505210685240754
        pole: -11,
        e0: (L::from_bits(0xBFE5, 0xD7322A62BB2CB4E0), L::from_bits(0x3F9F, 0xADF8DA0AE37FF91A)),
        sin_e0: (L::from_bits(0xBFE7, 0xA903B70102AB4D7C), L::from_bits(0xBFA5, 0xFE7BF764FAD59A38)),
        cot_e0: (L::from_bits(0xC016, 0xC1E078D7A3E28417), L::from_bits(0x3FD5, 0xA92349F22A1FEE6F)),
        c1: (L::from_bits(0x4000, 0x9C5491AA027EEBAD), L::from_bits(0x3FBF, 0xD8CD25977011E5DE)),
        c: &[
            L::from_bits(0xBFFA, 0xB1F99BE9110FBC00),
            L::from_bits(0xBFF5, 0xA4DF08081D1C2FA4),
            L::from_bits(0xBFF0, 0xE4F4941F0C05283B),
            L::from_bits(0xBFEC, 0xBEA6956243FC5323),
            L::from_bits(0xBFE8, 0xB048F44FC6887343),
            L::from_bits(0xBFE4, 0xAE899C105FC6147D),
            L::from_bits(0xBFE0, 0xB55654660F558552),
            L::from_bits(0xBFDC, 0xC342F1FE21E0E7B2),
            L::from_bits(0xBFD8, 0xD821EEF2EC9281BE),
            L::from_bits(0xBFD4, 0xF486B12F26AEC55C),
            L::from_bits(0xBFD1, 0x8CCDB3524B900B6B),
        ],
    },
    LdZero {
        // -11.99999999791232429020392
        pole: -12,
        e0: (L::from_bits(0x3FE2, 0x8F76C78C7821BE53), L::from_bits(0x3FA0, 0xF0B0DA1D09F18DCF)),
        sin_e0: (L::from_bits(0x3FE3, 0xE15A4A51FABCE460), L::from_bits(0xBFA2, 0xC2B5D000AA9B74A4)),
        cot_e0: (L::from_bits(0x401A, 0x916859FF94B3F59C), L::from_bits(0x3FD8, 0xC10A6B22CA3F1685)),
        c1: (L::from_bits(0x4000, 0xA1A9E6FCD383E799), L::from_bits(0xBFBF, 0xBC8F92D3338C91A0)),
        c: &[
            L::from_bits(0xBFFA, 0xA3C0B861CC9E3B64),
            L::from_bits(0xBFF5, 0x8B96571815870CCD),
            L::from_bits(0xBFF0, 0xB263323FE7C5F008),
            L::from_bits(0xBFEC, 0x88B62CFDC9B2785F),
            L::from_bits(0xBFE7, 0xE8B48EDF0299D15F),
            L::from_bits(0xBFE3, 0xD4163F4765D4A8B3),
            L::from_bits(0xBFDF, 0xCADADBD1C525656C),
            L::from_bits(0xBFDB, 0xC91B75CDFD5408D2),
            L::from_bits(0xBFD7, 0xCCF759B3213BC6B0),
            L::from_bits(0xBFD3, 0xD589D96C9BD78DF2),
            L::from_bits(0xBFCF, 0xE27997B7734BF44B),
        ],
    },
    LdZero {
        // -12.00000000208767568777754
        pole: -12,
        e0: (L::from_bits(0xBFE2, 0x8F76C7731567C0F0), L::from_bits(0xBFA0, 0x943CE1E4837D59D6)),
        sin_e0: (L::from_bits(0xBFE3, 0xE15A4A2A1A8FE662), L::from_bits(0x3F9E, 0x8DE13B97DEAD4795)),
        cot_e0: (L::from_bits(0xC01A, 0x91685A194F7950F4), L::from_bits(0xBFD9, 0xADC99372D833335C)),
        c1: (L::from_bits(0x4000, 0xA1A9E6FD2F488909), L::from_bits(0xBFBF, 0xED290352D614790F)),
        c: &[
            L::from_bits(0xBFFA, 0xA3C0B860E1F0FF37),
            L::from_bits(0xBFF5, 0x8B96571685A65594),
            L::from_bits(0xBFF0, 0xB263323CE9A1FFA5),
            L::from_bits(0xBFEC, 0x88B62CFABB3D7E24),
            L::from_bits(0xBFE7, 0xE8B48ED882A22D2A),
            L::from_bits(0xBFE3, 0xD4163F404AEEACC8),
            L::from_bits(0xBFDF, 0xCADADBC9D883B64A),
            L::from_bits(0xBFDB, 0xC91B75C5040AA818),
            L::from_bits(0xBFD7, 0xCCF759A8D8880628),
            L::from_bits(0xBFD3, 0xD589D960B5CF8778),
            L::from_bits(0xBFCF, 0xE27997A9937300BB),
        ],
    },
    LdZero {
        // -12.99999999983940956156466
        pole: -13,
        e0: (L::from_bits(0x3FDE, 0xB092309E80685A00), L::from_bits(0x3F9C, 0xE2A0AADCB5DBCCBB)),
        sin_e0: (L::from_bits(0x3FE0, 0x8AADB7899D104C09), L::from_bits(0xBF9F, 0xFC496DE73D8F450D)),
        cot_e0: (L::from_bits(0x401D, 0xEC499252912F1D13), L::from_bits(0x3FDB, 0xA1816598950ADED8)),
        c1: (L::from_bits(0x4000, 0xA69635C1EA704B30), L::from_bits(0x3FBC, 0xB99F70B765D10D17)),
        c: &[
            L::from_bits(0xBFFA, 0x97A26CA405A5A734),
            L::from_bits(0xBFF4, 0xEF66C94B182068A2),
            L::from_bits(0xBFF0, 0x8DAC8658B3C81598),
            L::from_bits(0xBFEB, 0xC92032C9D42811D1),
            L::from_bits(0xBFE7, 0x9E8DD642D71F78CF),
            L::from_bits(0xBFE3, 0x85DC84F6E97E2E96),
            L::from_bits(0xBFDE, 0xED39616C0DB76CC4),
            L::from_bits(0xBFDA, 0xD9E36816C0618939),
            L::from_bits(0xBFD6, 0xCDC1746BECBAE8A3),
            L::from_bits(0xBFD2, 0xC6A0A04C59852640),
            L::from_bits(0xBFCE, 0xC33648739B3A1309),
        ],
    },
    LdZero {
        // -13.00000000016059043830109
        pole: -13,
        e0: (L::from_bits(0xBFDE, 0xB092309C06683DD2), L::from_bits(0x3F9D, 0x8DF8391FEF50BD48)),
        sin_e0: (L::from_bits(0xBFE0, 0x8AADB787AB1EF271), L::from_bits(0x3F9F, 0x8BCA2091EFA18EE2)),
        cot_e0: (L::from_bits(0xC01D, 0xEC499255E19A7A40), L::from_bits(0xBFDC, 0xC1430812D622B333)),
        c1: (L::from_bits(0x4000, 0xA69635C1F0F9AF52), L::from_bits(0xBFBC, 0xF76177365AB3949B)),
        c: &[
            L::from_bits(0xBFFA, 0x97A26CA3F62AB629),
            L::from_bits(0xBFF4, 0xEF66C94AE744A6CD),
            L::from_bits(0xBFF0, 0x8DAC8658886E4662),
            L::from_bits(0xBFEB, 0xC92032C982230717),
            L::from_bits(0xBFE7, 0x9E8DD6428655EDC9),
            L::from_bits(0xBFE3, 0x85DC84F697AEB935),
            L::from_bits(0xBFDE, 0xED39616B64A58F8F),
            L::from_bits(0xBFDA, 0xD9E368160EFC69EC),
            L::from_bits(0xBFD6, 0xCDC1746B305B1AC8),
            L::from_bits(0xBFD2, 0xC6A0A04B8F8DF47F),
            L::from_bits(0xBFCE, 0xC3364872C0FAA3EA),
        ],
    },
    LdZero {
        // -13.99999999998852925440192
        pole: -14,
        e0: (L::from_bits(0x3FDA, 0xC9CBA5461E7B5C1F), L::from_bits(0x3F99, 0xC5DD12476E37013A)),
        sin_e0: (L::from_bits(0x3FDC, 0x9E7D6409F4FCC502), L::from_bits(0x3F9B, 0xF510CFE009FFC134)),
        cot_e0: (L::from_bits(0x4021, 0xCEC0600996FA9594), L::from_bits(0xBFE0, 0xF4B26BF265B96B71)),
        c1: (L::from_bits(0x4000, 0xAB287EE67FC67C87), L::from_bits(0x3FBD, 0xC562E5CCF3DA5900)),
        c: &[
            L::from_bits(0xBFFA, 0x8D2F7C5066E046FF),
            L::from_bits(0xBFF4, 0xCF8E9789335C5A9B),
            L::from_bits(0xBFEF, 0xE4C1DBF74B8C5E7C),
            L::from_bits(0xBFEB, 0x9736E1AAA325D8F8),
            L::from_bits(0xBFE6, 0xDE09ED6ED42CA2DB),
            L::from_bits(0xBFE2, 0xAE97FB25CB891D7F),
            L::from_bits(0xBFDE, 0x901852A403689FE9),
            L::from_bits(0xBFD9, 0xF68FD6B78E8CB283),
            L::from_bits(0xBFD5, 0xD8E3F6E22A9C7376),
            L::from_bits(0xBFD1, 0xC30D2749A06B916D),
            L::from_bits(0xBFCD, 0xB297829458F71801),
        ],
    },
    LdZero {
        // -14.00000000001147074559738
        pole: -14,
        e0: (L::from_bits(0xBFDA, 0xC9CBA545E94E75EC), L::from_bits(0xBF99, 0xAE31EEA7C4A03CEB)),
        sin_e0: (L::from_bits(0xBFDC, 0x9E7D6409CB393939), L::from_bits(0x3F92, 0x86A6C8931B586997)),
        cot_e0: (L::from_bits(0xC021, 0xCEC06009CD75CEDC), L::from_bits(0x3FDE, 0xC35B18FD98B2BE49)),
        c1: (L::from_bits(0x4000, 0xAB287EE68035C720), L::from_bits(0xBFBF, 0xFCCE756E025BACC3)),
        c: &[
            L::from_bits(0xBFFA, 0x8D2F7C5065EADCE5),
            L::from_bits(0xBFF4, 0xCF8E9789308B11DF),
            L::from_bits(0xBFEF, 0xE4C1DBF746E466AF),
            L::from_bits(0xBFEB, 0x9736E1AA9F0BB26C),
            L::from_bits(0xBFE6, 0xDE09ED6ECCA5DFD3),
            L::from_bits(0xBFE2, 0xAE97FB25C46FC1EC),
            L::from_bits(0xBFDE, 0x901852A3FC936C3F),
            L::from_bits(0xBFD9, 0xF68FD6B781315C12),
            L::from_bits(0xBFD5, 0xD8E3F6E21D65E8B4),
            L::from_bits(0xBFD1, 0xC30D27499338E8C6),
            L::from_bits(0xBFCD, 0xB29782944BAD794D),
        ],
    },
    LdZero {
        // -14.99999999999923528362682
        pole: -15,
        e0: (L::from_bits(0x3FD6, 0xD73F9F399FB10CFD), L::from_bits(0xBF95, 0xE3F13A6E6AF93C7C)),
        sin_e0: (L::from_bits(0x3FD8, 0xA90E489312B37BB0), L::from_bits(0xBF97, 0xD91E471636C536F4)),
        cot_e0: (L::from_bits(0x4025, 0xC1D45A091555F7DB), L::from_bits(0xBFE3, 0xB5801C988A444826)),
        c1: (L::from_bits(0x4000, 0xAF6CC32AC43EEDA2), L::from_bits(0xBFBF, 0x850052E09C534309)),
        c: &[
            L::from_bits(0xBFFA, 0x84155114190E4B6A),
            L::from_bits(0xBFF4, 0xB5AA8E5522C1E52E),
            L::from_bits(0xBFEF, 0xBB550070CA81385E),
            L::from_bits(0xBFEA, 0xE7BACE62112DCB30),
            L::from_bits(0xBFE6, 0x9F31F7414F80D655),
            L::from_bits(0xBFE1, 0xEA45F9A0959D9CAB),
            L::from_bits(0xBFDD, 0xB4EFDB975A935357),
            L::from_bits(0xBFD9, 0x90DEBC7671AAC048),
            L::from_bits(0xBFD4, 0xEE885F32D5AE302A),
            L::from_bits(0xBFD0, 0xC8C5673C772FC99A),
        ],
    },
    LdZero {
        // -15.00000000000076471637318
        pole: -15,
        e0: (L::from_bits(0xBFD6, 0xD73F9F399BD0E421), L::from_bits(0x3F91, 0xF42C239C9E8EDF17)),
        sin_e0: (L::from_bits(0xBFD8, 0xA90E48930FA83E29), L::from_bits(0xBF97, 0xDDD6842C29993A6A)),
        cot_e0: (L::from_bits(0xC025, 0xC1D45A0918D3664E), L::from_bits(0x3FE4, 0xA88F971EA3C04650)),
        c1: (L::from_bits(0x4000, 0xAF6CC32AC445DE8D), L::from_bits(0x3FBF, 0xCAB93BE9B8B3973F)),
        c: &[
            L::from_bits(0xBFFA, 0x8415511418FFF979),
            L::from_bits(0xBFF4, 0xB5AA8E55229A8471),
            L::from_bits(0xBFEF, 0xBB550070CA445508),
            L::from_bits(0xBFEA, 0xE7BACE6210C9674B),
            L::from_bits(0xBFE6, 0x9F31F7414F2AA886),
            L::from_bits(0xBFE1, 0xEA45F9A095057A42),
            L::from_bits(0xBFDD, 0xB4EFDB975A0A4A64),
            L::from_bits(0xBFD9, 0x90DEBC76712D6666),
            L::from_bits(0xBFD4, 0xEE885F32D4C61299),
            L::from_bits(0xBFD0, 0xC8C5673C7656C84F),
        ],
    },
    LdZero {
        // -15.99999999999995220522668
        pole: -16,
        e0: (L::from_bits(0x3FD2, 0xD73F9F399DE0AED2), L::from_bits(0xBF91, 0xE5C9A226C4D3E84D)),
        sin_e0: (L::from_bits(0x3FD4, 0xA90E48931146C4FE), L::from_bits(0xBF93, 0x962B64DEAF32E38C)),
        cot_e0: (L::from_bits(0x4029, 0xC1D45A0916F820A7), L::from_bits(0x3FE7, 0x93E69172EF8CFDB3)),
        c1: (L::from_bits(0x4000, 0xB36CC32AC44231ED), L::from_bits(0x3FBE, 0xCAB10A3841AFBB5E)),
        c: &[
            L::from_bits(0xBFF9, 0xF82AA228320F0F1A),
            L::from_bits(0xBFF4, 0xA05538FFCD59E4B0),
            L::from_bits(0xBFEF, 0x9B550070CA64422E),
            L::from_bits(0xBFEA, 0xB4879B2EDDCAB1EA),
            L::from_bits(0xBFE5, 0xE90E992D4959DDE4),
            L::from_bits(0xBFE1, 0xA121675770C254E6),
            L::from_bits(0xBFDC, 0xE9DFB72EB4A2D14B),
            L::from_bits(0xBFD8, 0xAFF65C7B1BC02D87),
            L::from_bits(0xBFD4, 0x8821F8CC6ED79EE9),
            L::from_bits(0xBFCF, 0xD75C42D604D33975),
        ],
    },
    LdZero {
        // -16.00000000000004779477332
        pole: -16,
        e0: (L::from_bits(0xBFD2, 0xD73F9F399DA1424C), L::from_bits(0x3F8D, 0xD88DC1D1060BA500)),
        sin_e0: (L::from_bits(0xBFD4, 0xA90E48931114F4DB), L::from_bits(0xBF93, 0x9ACEBB9FB466DD1A)),
        cot_e0: (L::from_bits(0xC029, 0xC1D45A0917313D81), L::from_bits(0xBFE8, 0xB2CF893CA68489C9)),
        c1: (L::from_bits(0x4000, 0xB36CC32AC4429A42), L::from_bits(0xBFBC, 0xFCFBDE9A4B050739)),
        c: &[
            L::from_bits(0xBFF9, 0xF82AA228320D7AAC),
            L::from_bits(0xBFF4, 0xA05538FFCD57DA44),
            L::from_bits(0xBFEF, 0x9B550070CA614B38),
            L::from_bits(0xBFEA, 0xB4879B2EDDC61A2B),
            L::from_bits(0xBFE5, 0xE90E992D49527529),
            L::from_bits(0xBFE1, 0xA121675770BC2FBE),
            L::from_bits(0xBFDC, 0xE9DFB72EB4986A2A),
            L::from_bits(0xBFD8, 0xAFF65C7B1BB73C48),
            L::from_bits(0xBFD4, 0x8821F8CC6ECFD70D),
            L::from_bits(0xBFCF, 0xD75C42D604C58D45),
        ],
    },
    LdZero {
        // -16.99999999999999718854275
        pole: -17,
        e0: (L::from_bits(0x3FCE, 0xCA963B81856C1E3C), L::from_bits(0xBF8D, 0xD41145C0115331D7)),
        sin_e0: (L::from_bits(0x3FD0, 0x9F1C808A6A86ED0B), L::from_bits(0xBF8F, 0xEEE77400E3DDD6CE)),
        cot_e0: (L::from_bits(0x402D, 0xCDF19FA9A8842788), L::from_bits(0x3FEC, 0xF92FE9A02DE29AFB)),
        c1: (L::from_bits(0x4000, 0xB73086EE880626F7), L::from_bits(0xBFBF, 0xB5B1390BBBC879EE)),
        c: &[
            L::from_bits(0xBFF9, 0xE9FE57BFAB698C95),
            L::from_bits(0xBFF4, 0x8E8C12D6FC39D9B0),
            L::from_bits(0xBFEF, 0x823906CDC1460948),
            L::from_bits(0xBFEA, 0x8EB7D4F41554D58F),
            L::from_bits(0xBFE5, 0xADBEA37BBE96D73C),
            L::from_bits(0xBFE0, 0xE290515430F5ECBC),
            L::from_bits(0xBFDC, 0x9B107D20415A4C41),
            L::from_bits(0xBFD7, 0xDC0F7AF5769BAE6E),
            L::from_bits(0xBFD3, 0xA09192E98B9C0252),
            L::from_bits(0xBFCE, 0xEF94FAEACDF82924),
        ],
    },
    LdZero {
        // -17.00000000000000281145725
        pole: -17,
        e0: (L::from_bits(0xBFCE, 0xCA963B8185688877), L::from_bits(0x3F8C, 0xD697166C4FA893D6)),
        sin_e0: (L::from_bits(0xBFD0, 0x9F1C808A6A841C3A), L::from_bits(0xBF8E, 0xB95C8F4C4750CB8A)),
        cot_e0: (L::from_bits(0xC02D, 0xCDF19FA9A887CC83), L::from_bits(0x3FEC, 0xECA0D683D672EEA7)),
        c1: (L::from_bits(0x4000, 0xB73086EE88062CC0), L::from_bits(0x3FBB, 0xA7951829AB0CAA3A)),
        c: &[
            L::from_bits(0xBFF9, 0xE9FE57BFAB69776F),
            L::from_bits(0xBFF4, 0x8E8C12D6FC39BFED),
            L::from_bits(0xBFEF, 0x823906CDC145E5FC),
            L::from_bits(0xBFEA, 0x8EB7D4F41554A1FF),
            L::from_bits(0xBFE5, 0xADBEA37BBE9688CB),
            L::from_bits(0xBFE0, 0xE290515430F57206),
            L::from_bits(0xBFDC, 0x9B107D204159EA4C),
            L::from_bits(0xBFD7, 0xDC0F7AF5769B0F99),
            L::from_bits(0xBFD3, 0xA09192E98B9B7FF9),
            L::from_bits(0xBFCE, 0xEF94FAEACDF7511A),
        ],
    },
    LdZero {
        // -17.99999999999999984380793
        pole: -18,
        e0: (L::from_bits(0x3FCA, 0xB413C31DCBECD2F7), L::from_bits(0x3F89, 0x9A91B3CF06F7C68E)),
        sin_e0: (L::from_bits(0x3FCC, 0x8D6EAB25B404F9D1), L::from_bits(0x3F8B, 0xE6BE2D4CC1521B93)),
        cot_e0: (L::from_bits(0x4031, 0xE7AFD39EDD969B8E), L::from_bits(0xBFF0, 0xF55551870296780E)),
        c1: (L::from_bits(0x4000, 0xBABEBFD2163F0D43), L::from_bits(0xBFBF, 0xBE03C5E49DEDCCDF)),
        c: &[
            L::from_bits(0xBFF9, 0xDD59FF413FF49256),
            L::from_bits(0xBFF3, 0xFF20CF2CF9BD3B8A),
            L::from_bits(0xBFEE, 0xDC7D9A44D998C720),
            L::from_bits(0xBFE9, 0xE49C88B6DF5B0584),
            L::from_bits(0xBFE5, 0x83A6FCAC90499DDD),
            L::from_bits(0xBFE0, 0xA26C46F3EA67DCCC),
            L::from_bits(0xBFDB, 0xD25AB138F8824436),
            L::from_bits(0xBFD7, 0x8D39EBEC586D95F9),
            L::from_bits(0xBFD2, 0xC300A75E1A2173B3),
            L::from_bits(0xBFCE, 0x89A7A6D4AC1ECDAC),
        ],
    },
    LdZero {
        // -18.00000000000000015619207
        pole: -18,
        e0: (L::from_bits(0xBFCA, 0xB413C31DCBECA4C4), L::from_bits(0x3F89, 0x9A00A6896D9A1CE4)),
        sin_e0: (L::from_bits(0xBFCC, 0x8D6EAB25B404D588), L::from_bits(0xBF89, 0x96031E9BFD0B0516)),
        cot_e0: (L::from_bits(0xC031, 0xE7AFD39EDD96D6FF), L::from_bits(0x3FEE, 0xF226E3930A4242D8)),
        c1: (L::from_bits(0x4000, 0xBABEBFD2163F0D90), L::from_bits(0x3FBF, 0xF65A173F2574CB9B)),
        c: &[
            L::from_bits(0xBFF9, 0xDD59FF413FF49149),
            L::from_bits(0xBFF3, 0xFF20CF2CF9BD391E),
            L::from_bits(0xBFEE, 0xDC7D9A44D998C3FC),
            L::from_bits(0xBFE9, 0xE49C88B6DF5B012C),
            L::from_bits(0xBFE5, 0x83A6FCAC90499ABD),
            L::from_bits(0xBFE0, 0xA26C46F3EA67D82D),
            L::from_bits(0xBFDB, 0xD25AB138F8823D3A),
            L::from_bits(0xBFD7, 0x8D39EBEC586D909D),
            L::from_bits(0xBFD2, 0xC300A75E1A216B61),
            L::from_bits(0xBFCE, 0x89A7A6D4AC1EC726),
        ],
    },
    LdZero {
        // -18.99999999999999999177936
        pole: -19,
        e0: (L::from_bits(0x3FC6, 0x97A4DA340A0ABA31), L::from_bits(0x3F84, 0x9B3471EDE8208B5A)),
        sin_e0: (L::from_bits(0x3FC7, 0xEE33A6FC21B76CE1), L::from_bits(0x3F83, 0xF0DE602AB4FCEA37)),
        cot_e0: (L::from_bits(0x4036, 0x899065A653917D10), L::from_bits(0xBFF5, 0xC18FE48C1A76C0C6)),
        c1: (L::from_bits(0x4000, 0xBE1D10A9AA74F275), L::from_bits(0x3FBD, 0xAE0688E19275E3AE)),
        c: &[
            L::from_bits(0xBFF9, 0xD2015ABBEE677358),
            L::from_bits(0xBFF3, 0xE5A61A5B6B0710DB),
            L::from_bits(0xBFEE, 0xBC4E65064046E202),
            L::from_bits(0xBFE9, 0xB93F490EFDE9FBDE),
            L::from_bits(0xBFE4, 0xCA71462EA0CC299D),
            L::from_bits(0xBFDF, 0xECFC1D8BAEA2DED9),
            L::from_bits(0xBFDB, 0x919D50F5124644A0),
            L::from_bits(0xBFD6, 0xB9883D4023C7B628),
            L::from_bits(0xBFD1, 0xF31823BA057E54FD),
            L::from_bits(0xBFCD, 0xA2D798869A1A4260),
        ],
    },
    LdZero {
        // -19.00000000000000000822064
        pole: -19,
        e0: (L::from_bits(0xBFC6, 0x97A4DA340A0AB81B), L::from_bits(0xBF85, 0xF63E3E0038E57F23)),
        sin_e0: (L::from_bits(0xBFC7, 0xEE33A6FC21B7699B), L::from_bits(0x3F85, 0xEA8BF326A432DCCE)),
        cot_e0: (L::from_bits(0xC036, 0x899065A653917EF4), L::from_bits(0x3FF5, 0x840C89A3D8E02155)),
        c1: (L::from_bits(0x4000, 0xBE1D10A9AA74F279), L::from_bits(0xBFBB, 0xE1DD79A7EB187ABB)),
        c: &[
            L::from_bits(0xBFF9, 0xD2015ABBEE67734B),
            L::from_bits(0xBFF3, 0xE5A61A5B6B0710BF),
            L::from_bits(0xBFEE, 0xBC4E65064046E1DF),
            L::from_bits(0xBFE9, 0xB93F490EFDE9FBB1),
            L::from_bits(0xBFE4, 0xCA71462EA0CC2960),
            L::from_bits(0xBFDF, 0xECFC1D8BAEA2DE83),
            L::from_bits(0xBFDB, 0x919D50F512464462),
            L::from_bits(0xBFD6, 0xB9883D4023C7B5CE),
            L::from_bits(0xBFD1, 0xF31823BA057E5479),
            L::from_bits(0xBFCD, 0xA2D798869A1A41FD),
        ],
    },
    LdZero {
        // -19.99999999999999999958897
        pole: -20,
        e0: (L::from_bits(0x3FC1, 0xF2A15D2010112853), L::from_bits(0x3F7B, 0xC6F9543D5D7EA412)),
        sin_e0: (L::from_bits(0x3FC3, 0xBE8FB8C9B492BC43), L::from_bits(0xBF82, 0xD71C5F8EB9E08C07)),
        cot_e0: (L::from_bits(0x403A, 0xABF47F0FE875DD73), L::from_bits(0xBFF9, 0x8FCF15A7125517A9)),
        c1: (L::from_bits(0x4000, 0xC15043DCDDA825AA), L::from_bits(0x3FBE, 0x8B8608549A9191C5)),
        c: &[
            L::from_bits(0xBFF9, 0xC7C3EA18175D35E1),
            L::from_bits(0xBFF3, 0xCFCDB2977E246B99),
            L::from_bits(0xBFEE, 0xA217821B2403B54C),
            L::from_bits(0xBFE9, 0x97B159CD920EE15B),
            L::from_bits(0xBFE4, 0x9DB4072CBBA80643),
            L::from_bits(0xBFDF, 0xAFA0CE7323EDA70C),
            L::from_bits(0xBFDA, 0xCD546661625BA185),
            L::from_bits(0xBFD5, 0xF8E57B29DE0B4A90),
            L::from_bits(0xBFD1, 0x9B2224341FEBFF3D),
            L::from_bits(0xBFCC, 0xC5BDBD61B7BCC231),
        ],
    },
    LdZero {
        // -20.00000000000000000041103
        pole: -20,
        e0: (L::from_bits(0xBFC1, 0xF2A15D2010112828), L::from_bits(0x3F80, 0xCCDD72B00976701D)),
        sin_e0: (L::from_bits(0xBFC3, 0xBE8FB8C9B492BC20), L::from_bits(0xBF82, 0xF7C76E50570AED5C)),
        cot_e0: (L::from_bits(0xC03A, 0xABF47F0FE875DD91), L::from_bits(0xBFF9, 0xF8CB8BEB223140F8)),
        c1: (L::from_bits(0x4000, 0xC15043DCDDA825AA), L::from_bits(0x3FBF, 0xA46D9340657002EE)),
        c: &[
            L::from_bits(0xBFF9, 0xC7C3EA18175D35E0),
            L::from_bits(0xBFF3, 0xCFCDB2977E246B98),
            L::from_bits(0xBFEE, 0xA217821B2403B54A),
            L::from_bits(0xBFE9, 0x97B159CD920EE159),
            L::from_bits(0xBFE4, 0x9DB4072CBBA80641),
            L::from_bits(0xBFDF, 0xAFA0CE7323EDA709),
            L::from_bits(0xBFDA, 0xCD546661625BA181),
            L::from_bits(0xBFD5, 0xF8E57B29DE0B4A8A),
            L::from_bits(0xBFD1, 0x9B2224341FEBFF39),
            L::from_bits(0xBFCC, 0xC5BDBD61B7BCC22C),
        ],
    },
    LdZero {
        // -20.99999999999999999998043
        pole: -21,
        e0: (L::from_bits(0x3FBD, 0xB8DC77B6E7AB8C60), L::from_bits(0x3F7C, 0x8AD8AD5212E8D195)),
        sin_e0: (L::from_bits(0x3FBF, 0x91308CCA7132D888), L::from_bits(0xBF7C, 0xDFE6CDDE04DF31FD)),
        cot_e0: (L::from_bits(0x403E, 0xE1B0E6C4E11AB2BA), L::from_bits(0xBFFC, 0xDE43C91B12DF69DB)),
        c1: (L::from_bits(0x4000, 0xC45C749FE9D8E8B6), L::from_bits(0x3FBF, 0xD47834A1EA5507E3)),
        c: &[
            L::from_bits(0xBFF9, 0xBE7A30EA3B5AE372),
            L::from_bits(0xBFF3, 0xBCEEC48375FF31D5),
            L::from_bits(0xBFEE, 0x8C8672043F46E0FE),
            L::from_bits(0xBFE8, 0xFACDD3154E341E9D),
            L::from_bits(0xBFE3, 0xF8A2E0FCB8F2895B),
            L::from_bits(0xBFDF, 0x8405EA4B789675F5),
            L::from_bits(0xBFDA, 0x93308B81D33CB566),
            L::from_bits(0xBFD5, 0xAA2556C385D69230),
            L::from_bits(0xBFD0, 0xCA44167CCE309B39),
            L::from_bits(0xBFCB, 0xF5DEF31DF8D1EA43),
        ],
    },
    LdZero {
        // -21.00000000000000000001957
        pole: -21,
        e0: (L::from_bits(0xBFBD, 0xB8DC77B6E7AB8C5F), L::from_bits(0x3F7C, 0xA84AB375365E8E86)),
        sin_e0: (L::from_bits(0xBFBF, 0x91308CCA7132D887), L::from_bits(0x3F7E, 0xBB5335198D21D7E8)),
        cot_e0: (L::from_bits(0xC03E, 0xE1B0E6C4E11AB2BC), L::from_bits(0x3FFD, 0x8712C03921BF4D5F)),
        c1: (L::from_bits(0x4000, 0xC45C749FE9D8E8B6), L::from_bits(0x3FBF, 0xD8C4938BD4948702)),
        c: &[
            L::from_bits(0xBFF9, 0xBE7A30EA3B5AE372),
            L::from_bits(0xBFF3, 0xBCEEC48375FF31D5),
            L::from_bits(0xBFEE, 0x8C8672043F46E0FE),
            L::from_bits(0xBFE8, 0xFACDD3154E341E9D),
            L::from_bits(0xBFE3, 0xF8A2E0FCB8F2895B),
            L::from_bits(0xBFDF, 0x8405EA4B789675F5),
            L::from_bits(0xBFDA, 0x93308B81D33CB566),
            L::from_bits(0xBFD5, 0xAA2556C385D69230),
            L::from_bits(0xBFD0, 0xCA44167CCE309B39),
            L::from_bits(0xBFCB, 0xF5DEF31DF8D1EA43),
        ],
    },
    LdZero {
        // -21.99999999999999999999911
        pole: -22,
        e0: (L::from_bits(0x3FB9, 0x8671CB6DBFC294A3), L::from_bits(0xBF78, 0xE5B1D35EB4978181)),
        sin_e0: (L::from_bits(0x3FBA, 0xD32F586C478FC696), L::from_bits(0x3F78, 0xF7D1D1B15FF8504D)),
        cot_e0: (L::from_bits(0x4043, 0x9B299EA75AC25AE0), L::from_bits(0x4002, 0xBB826754C70F887A)),
        c1: (L::from_bits(0x4000, 0xC7452ECE757BD171), L::from_bits(0xBFBF, 0xCC623BCE159F2DAB)),
        c: &[
            L::from_bits(0xBFF9, 0xB603B634480C9B83),
            L::from_bits(0xBFF3, 0xAC851C58E3F3036A),
            L::from_bits(0xBFED, 0xF53DA3AB9CFED112),
            L::from_bits(0xBFE8, 0xD1227A39843EA931),
            L::from_bits(0xBFE3, 0xC620C2DB09F78E6B),
            L::from_bits(0xBFDE, 0xC9132A0934F0AFBC),
            L::from_bits(0xBFD9, 0xD63BE0DAE5729291),
            L::from_bits(0xBFD4, 0xECAB5FBEF5A976D3),
            L::from_bits(0xBFD0, 0x8670C2FE7E9DE876),
            L::from_bits(0xBFCB, 0x9C2F2DF801D8ECF6),
        ],
    },
    LdZero {
        // -22.00000000000000000000089
        pole: -22,
        e0: (L::from_bits(0xBFB9, 0x8671CB6DBFC294A2), L::from_bits(0xBF78, 0xFED3434526706C71)),
        sin_e0: (L::from_bits(0xBFBA, 0xD32F586C478FC696), L::from_bits(0xBF78, 0xA17CFE1D01EBA2D7)),
        cot_e0: (L::from_bits(0xC043, 0x9B299EA75AC25AE0), L::from_bits(0xC002, 0xDB396762A34025F4)),
        c1: (L::from_bits(0x4000, 0xC7452ECE757BD171), L::from_bits(0xBFBF, 0xCC32706142BD14E4)),
        c: &[
            L::from_bits(0xBFF9, 0xB603B634480C9B83),
            L::from_bits(0xBFF3, 0xAC851C58E3F3036A),
            L::from_bits(0xBFED, 0xF53DA3AB9CFED112),
            L::from_bits(0xBFE8, 0xD1227A39843EA931),
            L::from_bits(0xBFE3, 0xC620C2DB09F78E6B),
            L::from_bits(0xBFDE, 0xC9132A0934F0AFBC),
            L::from_bits(0xBFD9, 0xD63BE0DAE5729291),
            L::from_bits(0xBFD4, 0xECAB5FBEF5A976D3),
            L::from_bits(0xBFD0, 0x8670C2FE7E9DE876),
            L::from_bits(0xBFCB, 0x9C2F2DF801D8ECF6),
        ],
    },
    LdZero {
        // -22.99999999999999999999996
        pole: -23,
        e0: (L::from_bits(0x3FB4, 0xBB0DA098B1C0CECC), L::from_bits(0xBF72, 0x8D6FE5AA56AB0430)),
        sin_e0: (L::from_bits(0x3FB6, 0x92E948A45E4DC1CD), L::from_bits(0xBF75, 0xAD06103C71FB87D0)),
        cot_e0: (L::from_bits(0x4047, 0xDF0BD410927762A3), L::from_bits(0xC006, 0xDCAA4669E5372AC5)),
        c1: (L::from_bits(0x4000, 0xCA0D87D996DFFDF6), L::from_bits(0x3FBE, 0xAA31AF8387CC5298)),
        c: &[
            L::from_bits(0xBFF9, 0xAE4586C76591B052),
            L::from_bits(0xBFF3, 0x9E280371386452D4),
            L::from_bits(0xBFED, 0xD743B27A36F66BAD),
            L::from_bits(0xBFE8, 0xAFC50BD18F56B4FE),
            L::from_bits(0xBFE3, 0x9F71A705C17F07B2),
            L::from_bits(0xBFDE, 0x9AF13EE7E01AFD94),
            L::from_bits(0xBFD9, 0x9E1296167E2BA36C),
            L::from_bits(0xBFD4, 0xA736C2F896179CFC),
            L::from_bits(0xBFCF, 0xB5E91AB8E782F594),
        ],
    },
    LdZero {
        // -23.00000000000000000000004
        pole: -23,
        e0: (L::from_bits(0xBFB4, 0xBB0DA098B1C0CECC), L::from_bits(0x3F72, 0x90CEE2F5D0B69A9A)),
        sin_e0: (L::from_bits(0xBFB6, 0x92E948A45E4DC1CD), L::from_bits(0x3F75, 0xAE58F565823ABF72)),
        cot_e0: (L::from_bits(0xC047, 0xDF0BD410927762A3), L::from_bits(0x4006, 0xDAA7C06E71F64A7D)),
        c1: (L::from_bits(0x4000, 0xCA0D87D996DFFDF6), L::from_bits(0x3FBE, 0xAA35AA340A438155)),
        c: &[
            L::from_bits(0xBFF9, 0xAE4586C76591B052),
            L::from_bits(0xBFF3, 0x9E280371386452D4),
            L::from_bits(0xBFED, 0xD743B27A36F66BAD),
            L::from_bits(0xBFE8, 0xAFC50BD18F56B4FE),
            L::from_bits(0xBFE3, 0x9F71A705C17F07B2),
            L::from_bits(0xBFDE, 0x9AF13EE7E01AFD94),
            L::from_bits(0xBFD9, 0x9E1296167E2BA36C),
            L::from_bits(0xBFD4, 0xA736C2F896179CFC),
            L::from_bits(0xBFCF, 0xB5E91AB8E782F594),
        ],
    },
    LdZero {
        // -24.0
        pole: -24,
        e0: (L::from_bits(0x3FAF, 0xF96780CB97ABBE65), L::from_bits(0x3F6D, 0x96991963B8280036)),
        sin_e0: (L::from_bits(0x3FB1, 0xC3E1B6307DBD0266), L::from_bits(0x3F6F, 0xDB946146EBEBD7BB)),
        cot_e0: (L::from_bits(0x404C, 0xA748DF0C6DD989FA), L::from_bits(0xC009, 0x931B9F4A91971CBF)),
        c1: (L::from_bits(0x4000, 0xCCB83284418AA8A1), L::from_bits(0xBFBE, 0xAB21BCD4DDAF320A)),
        c: &[
            L::from_bits(0xBFF9, 0xA7291500491FE936),
            L::from_bits(0xBFF3, 0x9183AAF2CCEF62A1),
            L::from_bits(0xBFED, 0xBDFB017D600C8B48),
            L::from_bits(0xBFE8, 0x94CCD790AA18F8D7),
            L::from_bits(0xBFE3, 0x817A5084C2C8A869),
            L::from_bits(0xBFDD, 0xF164013B30DE45F0),
            L::from_bits(0xBFD8, 0xEC3C457FAA70F36D),
            L::from_bits(0xBFD3, 0xEFB84E9F0401CD9D),
            L::from_bits(0xBFCE, 0xFA2BF30F386902BC),
        ],
    },
    LdZero {
        // -24.0
        pole: -24,
        e0: (L::from_bits(0xBFAF, 0xF96780CB97ABBE65), L::from_bits(0xBF6D, 0x966885C6BE008167)),
        sin_e0: (L::from_bits(0xBFB1, 0xC3E1B6307DBD0266), L::from_bits(0xBF6F, 0xDB6E3A5E88D8BF97)),
        cot_e0: (L::from_bits(0xC04C, 0xA748DF0C6DD989FA), L::from_bits(0x4009, 0x92DA753E73F14306)),
        c1: (L::from_bits(0x4000, 0xCCB83284418AA8A1), L::from_bits(0xBFBE, 0xAB21941E3AEBA4B3)),
        c: &[
            L::from_bits(0xBFF9, 0xA7291500491FE936),
            L::from_bits(0xBFF3, 0x9183AAF2CCEF62A1),
            L::from_bits(0xBFED, 0xBDFB017D600C8B48),
            L::from_bits(0xBFE8, 0x94CCD790AA18F8D7),
            L::from_bits(0xBFE3, 0x817A5084C2C8A869),
            L::from_bits(0xBFDD, 0xF164013B30DE45F0),
            L::from_bits(0xBFD8, 0xEC3C457FAA70F36D),
            L::from_bits(0xBFD3, 0xEFB84E9F0401CD9D),
            L::from_bits(0xBFCE, 0xFA2BF30F386902BC),
        ],
    },
    LdZero {
        // -25.0
        pole: -25,
        e0: (L::from_bits(0x3FAB, 0x9F9E66E8B2FD46A7), L::from_bits(0x3F69, 0x8948D41972ADA531)),
        sin_e0: (L::from_bits(0x3FAC, 0xFABA82CD6DBEBB64), L::from_bits(0x3F69, 0xEA42C18BD6249408)),
        cot_e0: (L::from_bits(0x4051, 0x82B0EE41B5D1F3CB), L::from_bits(0x4010, 0x834AB603F6CE85D9)),
        c1: (L::from_bits(0x4000, 0xCF478EAD374D37FD), L::from_bits(0xBFB9, 0xE953E08C6CE30B34)),
        c: &[
            L::from_bits(0xBFF9, 0xA09B5C45820F1E0C),
            L::from_bits(0xBFF3, 0x86545B3253A659D2),
            L::from_bits(0xBFED, 0xA881729B2F805168),
            L::from_bits(0xBFE7, 0xFD9EAF5E6168C6ED),
            L::from_bits(0xBFE2, 0xD40B07B10B21233A),
            L::from_bits(0xBFDD, 0xBDEBC2509BE8515A),
            L::from_bits(0xBFD8, 0xB296E04FF95D7B16),
            L::from_bits(0xBFD3, 0xAE21ABB4FBF4C0B6),
            L::from_bits(0xBFCE, 0xAE9D22E6FDFBC4C5),
        ],
    },
    LdZero {
        // -25.0
        pole: -25,
        e0: (L::from_bits(0xBFAB, 0x9F9E66E8B2FD46A7), L::from_bits(0xBF69, 0x894791C449953D1F)),
        sin_e0: (L::from_bits(0xBFAC, 0xFABA82CD6DBEBB64), L::from_bits(0xBF69, 0xEA3ECCE887FBA7AA)),
        cot_e0: (L::from_bits(0xC051, 0x82B0EE41B5D1F3CB), L::from_bits(0xC010, 0x834B39F9461CD773)),
        c1: (L::from_bits(0x4000, 0xCF478EAD374D37FD), L::from_bits(0xBFB9, 0xE953AE7A7D8209F0)),
        c: &[
            L::from_bits(0xBFF9, 0xA09B5C45820F1E0C),
            L::from_bits(0xBFF3, 0x86545B3253A659D2),
            L::from_bits(0xBFED, 0xA881729B2F805168),
            L::from_bits(0xBFE7, 0xFD9EAF5E6168C6ED),
            L::from_bits(0xBFE2, 0xD40B07B10B21233A),
            L::from_bits(0xBFDD, 0xBDEBC2509BE8515A),
            L::from_bits(0xBFD8, 0xB296E04FF95D7B16),
            L::from_bits(0xBFD3, 0xAE21ABB4FBF4C0B6),
            L::from_bits(0xBFCE, 0xAE9D22E6FDFBC4C5),
        ],
    },
    LdZero {
        // -26.0
        pole: -26,
        e0: (L::from_bits(0x3FA6, 0xC4742FE35272CD1C), L::from_bits(0x3F65, 0xF2050F82D1AD2538)),
        sin_e0: (L::from_bits(0x3FA8, 0x9A4B642FA5FF383E), L::from_bits(0xBF67, 0xC844CAA033F4B5D0)),
        cot_e0: (L::from_bits(0x4055, 0xD45F832AC7752C2A), L::from_bits(0x4014, 0x9559CED18DABE827)),
        c1: (L::from_bits(0x4000, 0xD1BDB60FAD749A73), L::from_bits(0x3FBE, 0x963F3A5A049727B0)),
        c: &[
            L::from_bits(0xBFF9, 0x9A8C3666D55F66C2),
            L::from_bits(0xBFF2, 0xF8C5C3F2D98F4662),
            L::from_bits(0xBFED, 0x96261CA84A33EC8D),
            L::from_bits(0xBFE7, 0xD9789BC6F483920B),
            L::from_bits(0xBFE2, 0xAEF7AB64812F7D65),
            L::from_bits(0xBFDD, 0x96CEE52A1002B61A),
            L::from_bits(0xBFD8, 0x8877CAC4003ED3E5),
            L::from_bits(0xBFD3, 0x800CCAD5D588D31A),
            L::from_bits(0xBFCD, 0xF723A655C65B247E),
        ],
    },
    LdZero {
        // -26.0
        pole: -26,
        e0: (L::from_bits(0xBFA6, 0xC4742FE35272CD1C), L::from_bits(0xBF65, 0xF20507CA8E7C0396)),
        sin_e0: (L::from_bits(0xBFA8, 0x9A4B642FA5FF383E), L::from_bits(0x3F67, 0xC844D0B05B1A05ED)),
        cot_e0: (L::from_bits(0xC055, 0xD45F832AC7752C2A), L::from_bits(0xC014, 0x9559D729F5528F74)),
        c1: (L::from_bits(0x4000, 0xD1BDB60FAD749A73), L::from_bits(0x3FBE, 0x963F3A68D7C6EABB)),
        c: &[
            L::from_bits(0xBFF9, 0x9A8C3666D55F66C2),
            L::from_bits(0xBFF2, 0xF8C5C3F2D98F4662),
            L::from_bits(0xBFED, 0x96261CA84A33EC8D),
            L::from_bits(0xBFE7, 0xD9789BC6F483920B),
            L::from_bits(0xBFE2, 0xAEF7AB64812F7D65),
            L::from_bits(0xBFDD, 0x96CEE52A1002B61A),
            L::from_bits(0xBFD8, 0x8877CAC4003ED3E5),
            L::from_bits(0xBFD3, 0x800CCAD5D588D31A),
            L::from_bits(0xBFCD, 0xF723A655C65B247E),
        ],
    },
    LdZero {
        // -27.0
        pole: -27,
        e0: (L::from_bits(0x3FA1, 0xE8D58E16E6751905), L::from_bits(0x3F60, 0x9A18F1891FE4EAFF)),
        sin_e0: (L::from_bits(0x3FA3, 0xB6DE17EC9ECFAAF4), L::from_bits(0xBF62, 0xA180F39B826330A1)),
        cot_e0: (L::from_bits(0x405A, 0xB33096AC184ADD44), L::from_bits(0xC019, 0xA1FC361BA6EF1C43)),
        c1: (L::from_bits(0x4000, 0xD41C86A7619A877D), L::from_bits(0xBFBF, 0xBE5BA52E3B5D2135)),
        c: &[
            L::from_bits(0xBFF9, 0x94EDD62EA59D34E5),
            L::from_bits(0xBFF2, 0xE703C9937206C211),
            L::from_bits(0xBFED, 0x865D3E1A98D70529),
            L::from_bits(0xBFE7, 0xBB89EC44B1749518),
            L::from_bits(0xBFE2, 0x916795C115DE5100),
            L::from_bits(0xBFDC, 0xF18D5D595E5C6F77),
            L::from_bits(0xBFD7, 0xD2A5A94196791378),
            L::from_bits(0xBFD2, 0xBE7A966F2EBCC24F),
            L::from_bits(0xBFCD, 0xB124B83763DE75AC),
        ],
    },
    LdZero {
        // -27.0
        pole: -27,
        e0: (L::from_bits(0xBFA1, 0xE8D58E16E6751905), L::from_bits(0xBF60, 0x9A18F13165097E42)),
        sin_e0: (L::from_bits(0xBFA3, 0xB6DE17EC9ECFAAF4), L::from_bits(0x3F62, 0xA180F3E06988588B)),
        cot_e0: (L::from_bits(0xC05A, 0xB33096AC184ADD44), L::from_bits(0x4019, 0xA1FC35D8228A2ED2)),
        c1: (L::from_bits(0x4000, 0xD41C86A7619A877D), L::from_bits(0xBFBF, 0xBE5BA52DF7A33DE2)),
        c: &[
            L::from_bits(0xBFF9, 0x94EDD62EA59D34E5),
            L::from_bits(0xBFF2, 0xE703C9937206C211),
            L::from_bits(0xBFED, 0x865D3E1A98D70529),
            L::from_bits(0xBFE7, 0xBB89EC44B1749518),
            L::from_bits(0xBFE2, 0x916795C115DE5100),
            L::from_bits(0xBFDC, 0xF18D5D595E5C6F77),
            L::from_bits(0xBFD7, 0xD2A5A94196791378),
            L::from_bits(0xBFD2, 0xBE7A966F2EBCC24F),
            L::from_bits(0xBFCD, 0xB124B83763DE75AC),
        ],
    },
    LdZero {
        // -28.0
        pole: -28,
        e0: (L::from_bits(0x3F9D, 0x850C5131A842E9BA), L::from_bits(0xBF5A, 0xE8EB8F273739FD81)),
        sin_e0: (L::from_bits(0x3F9E, 0xD0FDD232FEA43116), L::from_bits(0x3F5D, 0xFE480E03C07A94FF)),
        cot_e0: (L::from_bits(0x405F, 0x9CCA83D69541819B), L::from_bits(0x401D, 0xE486A1888DC0A90C)),
        c1: (L::from_bits(0x4000, 0xD665AB39AABF19C6), L::from_bits(0xBFBE, 0xEA6E25C9EC3149C4)),
        c: &[
            L::from_bits(0xBFF9, 0x8FB45E04D9DBE687),
            L::from_bits(0xBFF2, 0xD717B0B28B275954),
            L::from_bits(0xBFEC, 0xF16EE3D8382F0DA0),
            L::from_bits(0xBFE7, 0xA29543B52C6E31B7),
            L::from_bits(0xBFE1, 0xF3464BF6E5368F60),
            L::from_bits(0xBFDC, 0xC2FCD5F581C4336D),
            L::from_bits(0xBFD7, 0xA41521DDB9E0D76E),
            L::from_bits(0xBFD2, 0x8F2CD811DCDD0759),
            L::from_bits(0xBFCD, 0x807CFBBA25994057),
        ],
    },
    LdZero {
        // -28.0
        pole: -28,
        e0: (L::from_bits(0xBF9D, 0x850C5131A842E9BA), L::from_bits(0x3F5A, 0xE8EB8F2E745BED59)),
        sin_e0: (L::from_bits(0xBF9E, 0xD0FDD232FEA43116), L::from_bits(0xBF5D, 0xFE480E00E8C16086)),
        cot_e0: (L::from_bits(0xC05F, 0x9CCA83D69541819B), L::from_bits(0xC01D, 0xE486A18CD1AB138F)),
        c1: (L::from_bits(0x4000, 0xD665AB39AABF19C6), L::from_bits(0xBFBE, 0xEA6E25C9E7864FD6)),
        c: &[
            L::from_bits(0xBFF9, 0x8FB45E04D9DBE687),
            L::from_bits(0xBFF2, 0xD717B0B28B275954),
            L::from_bits(0xBFEC, 0xF16EE3D8382F0DA0),
            L::from_bits(0xBFE7, 0xA29543B52C6E31B7),
            L::from_bits(0xBFE1, 0xF3464BF6E5368F60),
            L::from_bits(0xBFDC, 0xC2FCD5F581C4336D),
            L::from_bits(0xBFD7, 0xA41521DDB9E0D76E),
            L::from_bits(0xBFD2, 0x8F2CD811DCDD0759),
            L::from_bits(0xBFCD, 0x807CFBBA25994057),
        ],
    },
    LdZero {
        // -29.0
        pole: -29,
        e0: (L::from_bits(0x3F98, 0x92CFCC5A1AC56BD6), L::from_bits(0xBF54, 0xE78C44C82F7A4EE5)),
        sin_e0: (L::from_bits(0x3F99, 0xE69C7E034DF2F85F), L::from_bits(0x3F58, 0xE39EF4F9CC762384)),
        cot_e0: (L::from_bits(0x4064, 0x8E17877A77435D75), L::from_bits(0xC023, 0xA872FECD30FFE6CB)),
        c1: (L::from_bits(0x4000, 0xD89AA265CE0E8C88), L::from_bits(0xBFBB, 0xB48BA9E561255CC3)),
        c: &[
            L::from_bits(0xBFF9, 0x8AD58BFBB8121567),
            L::from_bits(0xBFF2, 0xC8C2E29D8EB81259),
            L::from_bits(0xBFEC, 0xD9B6629B004ABB73),
            L::from_bits(0xBFE7, 0x8DA4B55F5A6DE92D),
            L::from_bits(0xBFE1, 0xCCC3CD8E07C92A56),
            L::from_bits(0xBFDC, 0x9E908BCC558A220C),
            L::from_bits(0xBFD7, 0x80EA5EAD253ED84E),
            L::from_bits(0xBFD1, 0xD95D18B7ACB71F20),
            L::from_bits(0xBFCC, 0xBC772C1C68A5E071),
        ],
    },
    LdZero {
        // -29.0
        pole: -29,
        e0: (L::from_bits(0xBF98, 0x92CFCC5A1AC56BD6), L::from_bits(0x3F54, 0xE78C44C8BDF3DAB4)),
        sin_e0: (L::from_bits(0xBF99, 0xE69C7E034DF2F85F), L::from_bits(0xBF58, 0xE39EF4F9B07C9320)),
        cot_e0: (L::from_bits(0xC064, 0x8E17877A77435D75), L::from_bits(0x4023, 0xA872FECD1FC347BF)),
        c1: (L::from_bits(0x4000, 0xD89AA265CE0E8C88), L::from_bits(0xBFBB, 0xB48BA9E55FE6E2D6)),
        c: &[
            L::from_bits(0xBFF9, 0x8AD58BFBB8121567),
            L::from_bits(0xBFF2, 0xC8C2E29D8EB81259),
            L::from_bits(0xBFEC, 0xD9B6629B004ABB73),
            L::from_bits(0xBFE7, 0x8DA4B55F5A6DE92D),
            L::from_bits(0xBFE1, 0xCCC3CD8E07C92A56),
            L::from_bits(0xBFDC, 0x9E908BCC558A220C),
            L::from_bits(0xBFD7, 0x80EA5EAD253ED84E),
            L::from_bits(0xBFD1, 0xD95D18B7ACB71F20),
            L::from_bits(0xBFCC, 0xBC772C1C68A5E071),
        ],
    },
    LdZero {
        // -30.0
        pole: -30,
        e0: (L::from_bits(0x3F93, 0x9C9962823EB07306), L::from_bits(0x3F52, 0xADED4C298A174E73)),
        sin_e0: (L::from_bits(0x3F94, 0xF5FC4225A87AA288), L::from_bits(0xBF50, 0xF22B08BDE6DECA79)),
        cot_e0: (L::from_bits(0x4069, 0x85360F02CFCF279D), L::from_bits(0x4028, 0xC214311FA9DEE1DC)),
        c1: (L::from_bits(0x4000, 0xDABCC487F030AEAA), L::from_bits(0x3FBD, 0xE3EE2697B8EE408B)),
        c: &[
            L::from_bits(0xBFF9, 0x8648765D9162DDA7),
            L::from_bits(0xBFF2, 0xBBD0DE03871551A2),
            L::from_bits(0xBFEC, 0xC4FFF4D7C0DFED81),
            L::from_bits(0xBFE6, 0xF7EFF0451C40D946),
            L::from_bits(0xBFE1, 0xAD57D277473F6961),
            L::from_bits(0xBFDC, 0x81D60CA196E04D0F),
            L::from_bits(0xBFD6, 0xCC345881F7A711F6),
            L::from_bits(0xBFD1, 0xA6848B97217DA526),
            L::from_bits(0xBFCC, 0x8BA748780BF3D1BF),
        ],
    },
    LdZero {
        // -30.0
        pole: -30,
        e0: (L::from_bits(0xBF93, 0x9C9962823EB07306), L::from_bits(0xBF52, 0xADED4C2989739AF0)),
        sin_e0: (L::from_bits(0xBF94, 0xF5FC4225A87AA288), L::from_bits(0x3F50, 0xF22B08BDEEE7EBCD)),
        cot_e0: (L::from_bits(0xC069, 0x85360F02CFCF279D), L::from_bits(0xC028, 0xC214311FAA6A2283)),
        c1: (L::from_bits(0x4000, 0xDABCC487F030AEAA), L::from_bits(0x3FBD, 0xE3EE2697B8F0D1B0)),
        c: &[
            L::from_bits(0xBFF9, 0x8648765D9162DDA7),
            L::from_bits(0xBFF2, 0xBBD0DE03871551A2),
            L::from_bits(0xBFEC, 0xC4FFF4D7C0DFED81),
            L::from_bits(0xBFE6, 0xF7EFF0451C40D946),
            L::from_bits(0xBFE1, 0xAD57D277473F6961),
            L::from_bits(0xBFDC, 0x81D60CA196E04D0F),
            L::from_bits(0xBFD6, 0xCC345881F7A711F6),
            L::from_bits(0xBFD1, 0xA6848B97217DA526),
            L::from_bits(0xBFCC, 0x8BA748780BF3D1BF),
        ],
    },
    LdZero {
        // -31.0
        pole: -31,
        e0: (L::from_bits(0x3F8E, 0xA1A6973C1FADE217), L::from_bits(0x3F4A, 0xF7237D35FE328C65)),
        sin_e0: (L::from_bits(0x3F8F, 0xFDEB9F1E9D65D110), L::from_bits(0x3F4E, 0xE902B48CA7E6FC42)),
        cot_e0: (L::from_bits(0x406E, 0x810C5E8AB950AE60), L::from_bits(0x402D, 0xEC038F96ACD12BD9)),
        c1: (L::from_bits(0x4000, 0xDCCD48A8F872BF2E), L::from_bits(0x3FBE, 0xF6181B8DECFBE076)),
        c: &[
            L::from_bits(0xBFF9, 0x820555111D3D9244),
            L::from_bits(0xBFF2, 0xB015538EAA779AE3),
            L::from_bits(0xBFEC, 0xB2D55001C61D4628),
            L::from_bits(0xBFE6, 0xD9EEE00662044F9D),
            L::from_bits(0xBFE1, 0x938874A9EBDC8A5C),
            L::from_bits(0xBFDB, 0xD5FFCCEE3260AEBA),
            L::from_bits(0xBFD6, 0xA2F38F6F0416A073),
            L::from_bits(0xBFD1, 0x80AA5CBA0AF39A96),
            L::from_bits(0xBFCB, 0xD0F994B37080BFC4),
        ],
    },
];

/// `log|gamma(x)|` and the sign of `gamma(x)` (musl's ld80 `__lgammal_r`).
#[allow(clippy::too_many_lines)]
fn lgammal_core(x: L) -> (L, i32) {
    let ix = ix_of(x);
    let neg = x.is_sign_negative();
    let mut sg = 1;
    // Purge off +-inf, NaN, +-0, tiny and negative arguments.
    if ix >= 0x7FFF_0000 {
        return (x * x, sg);
    }
    if ix < 0x3FC0_8000 {
        // |x| < 2^-63: -log(|x|)
        if neg {
            sg = -1;
        }
        return (-logl_raw(x.abs()), sg);
    }
    let mut x = x;
    let mut nadj = ZERO;
    if neg {
        x = -x;
        let mut t = sin_pi(x);
        if t.is_zero() {
            // A non-positive integer: a pole, +inf in every direction
            // (`1 / (x - x)` is -inf downward).
            return (ONE / ZERO, sg);
        }
        if t > ZERO {
            sg = -1;
        } else {
            t = -t;
        }
        if let Some(r) = lgammal_near_zero(-x) {
            return (r, sg);
        }
        nadj = logl_raw(LG_PI / (t * x));
    }
    let ix = ix_of(x);
    let r;
    // Purge off 1 and 2 (so the sign is right with downward rounding).
    if (ix == 0x3FFF_8000 || ix == 0x4000_8000) && x.significand << 1 == 0 {
        r = ZERO;
    } else if ix < 0x4000_8000 {
        // x < 2.0
        let (mut acc, y, i);
        if ix <= 0x3FFE_E666 {
            // x <= 0.9: lgamma(x) = lgamma(x+1) - log(x)
            acc = -logl_raw(x);
            if ix >= 0x3FFE_BB4A {
                y = x - ONE;
                i = 0;
            } else if ix >= 0x3FFC_ED33 {
                y = x - (LG_TC - ONE);
                i = 1;
            } else {
                y = x;
                i = 2;
            }
        } else {
            acc = ZERO;
            if ix >= 0x3FFF_DDA6 {
                // [1.7316, 2]
                y = x - ld(2.0);
                i = 0;
            } else if ix >= 0x3FFF_9DA6 {
                // [1.23, 1.73]
                y = x - LG_TC;
                i = 1;
            } else {
                // [0.9, 1.23]
                y = x - ONE;
                i = 2;
            }
        }
        match i {
            0 => {
                let p1 = horner_l(y, &[LG_A0, LG_A1, LG_A2, LG_A3, LG_A4, LG_A5]);
                let p2 = horner1_l(y, &[LG_B0, LG_B1, LG_B2, LG_B3, LG_B4]);
                acc = acc + (ld(0.5) * y + y * p1 / p2);
            }
            1 => {
                let p1 = horner_l(y, &[LG_G0, LG_G1, LG_G2, LG_G3, LG_G4, LG_G5, LG_G6]);
                let p2 = horner1_l(y, &[LG_H0, LG_H1, LG_H2, LG_H3, LG_H4, LG_H5]);
                let p = LG_TT + y * p1 / p2;
                acc = acc + (LG_TF + p);
            }
            _ => {
                let p1 = y * horner_l(y, &[LG_U0, LG_U1, LG_U2, LG_U3, LG_U4, LG_U5, LG_U6]);
                let p2 = horner1_l(y, &[LG_V0, LG_V1, LG_V2, LG_V3, LG_V4, LG_V5]);
                acc = acc + (ld(-0.5) * y + p1 / p2);
            }
        }
        r = acc;
    } else if ix < 0x4002_8000 {
        // x < 8.0
        #[allow(clippy::cast_possible_truncation)]
        let i = x.round_int_toward(3).to_i64_rint() as i32;
        let y = x - L::from_i64(i64::from(i));
        let p = y * horner_l(y, &[LG_S0, LG_S1, LG_S2, LG_S3, LG_S4, LG_S5, LG_S6]);
        let q = horner1_l(y, &[LG_R0, LG_R1, LG_R2, LG_R3, LG_R4, LG_R5, LG_R6]);
        let mut acc = ld(0.5) * y + p / q;
        // lgamma(1+s) = log(s) + lgamma(s)
        let mut z = ONE;
        for k in (2..i).rev() {
            z = z * (y + L::from_i64(i64::from(k)));
        }
        if i >= 3 {
            acc = acc + logl_raw(z);
        }
        r = acc;
    } else if ix < 0x4041_8000 {
        // 8.0 <= x < 2^66
        let t = logl_raw(x);
        let z = ONE / x;
        let y = z * z;
        let w = LG_W0 + z * horner_l(y, &[LG_W1, LG_W2, LG_W3, LG_W4, LG_W5, LG_W6, LG_W7]);
        r = (x - ld(0.5)) * (t - ONE) + w;
    } else {
        // 2^66 <= x <= inf
        r = x * (logl_raw(x) - ONE);
    }
    (if neg { nadj - r } else { r }, sg)
}

/// glibc's `w_lgamma_template.c`: `ERANGE` when a finite argument gives an
/// infinity -- a pole at 0 or a negative integer, reported here, or an
/// overflow, [`ranged`]'s.
fn lgamma_pole(x: L) -> bool {
    x.is_finite() && x <= ZERO && floorl(x) == x
}

/// `(log|gamma(x)|, its sign)`, with `ERANGE`: computed to nearest, since the
/// expansion about each zero below -2 (`lgammal_near_zero`) carries exact
/// sums and products, which are exact only there.
fn lgammal_ranged(x: L) -> (L, i32) {
    if lgamma_pole(x) {
        set(errno::ERANGE);
    }
    let sign = core::cell::Cell::new(1);
    let y = ranged(
        x,
        |x| {
            let (y, s) = crate::fenv::in_nearest_x87(x, lgammal_core);
            sign.set(s);
            y
        },
        |y| overflow_only(x.is_finite() && !lgamma_pole(x), y),
    );
    (y, sign.get())
}

/// `log|gamma(x)|`, its sign stored in `signgam` -- the one C declares, the
/// double functions' too (`crate::math::signgam`).
#[must_use]
pub fn lgammal(x: L) -> L {
    let (y, sg) = lgammal_ranged(x);
    crate::math::signgam.store(sg, core::sync::atomic::Ordering::Relaxed);
    y
}

/// `log|gamma(x)|` and its sign, without touching `signgam`.
#[must_use]
pub fn lgammal_r(x: L) -> (L, i32) {
    lgammal_ranged(x)
}

const TG_P: [L; 8] = [
    L::from_bits(0x3FF0, 0xB0B22BDA3F22434A), // 4.212760487471622013093E-5L
    L::from_bits(0x3FF3, 0xEE2E335BE82FF5AA), // 4.542931960608009155600E-4L
    L::from_bits(0x3FF7, 0x861BC7173757BE6C), // 4.092666828394035500949E-3L
    L::from_bits(0x3FF9, 0xC368B16651967F43), // 2.385363243461108252554E-2L
    L::from_bits(0x3FFB, 0xE3F48C3A8EB59549), // 1.113062816019361559013E-1L
    L::from_bits(0x3FFD, 0xB9D4C8E423AF8D75), // 3.629515436640239168939E-1L
    L::from_bits(0x3FFE, 0xD67A16C819B329CF), // 8.378004301573126728826E-1L
    L::from_bits(0x3FFF, 0x8000000000000000), // 1.000000000000000000009E0L
];
const TG_Q: [L; 9] = [
    L::from_bits(0xBFEE, 0xEA6712682DE85473), // -1.397148517476170440917E-5L
    L::from_bits(0x3FF2, 0xF60EA2DDC2F0334B), // 2.346584059160635244282E-4L
    L::from_bits(0xBFF5, 0xA23DA6911853BEED), // -1.237799246653152231188E-3L
    L::from_bits(0xBFF4, 0xD08F5DFD7CB1296E), // -7.955933682494738320586E-4L
    L::from_bits(0x3FF9, 0xE338D7BC79890417), // 2.773706565840072979165E-2L
    L::from_bits(0xBFFA, 0xBDCDD58036983295), // -4.633887671244534213831E-2L
    L::from_bits(0xBFFC, 0xE5BC4AD33AB775EF), // -2.243510905670329164562E-1L
    L::from_bits(0x3FFD, 0xD47CFD572EC7E458), // 4.150160950588455434583E-1L
    L::from_bits(0x3FFF, 0x8000000000000000), // 9.999999999999999999908E-1L
];
const TG_STIR: [L; 9] = [
    L::from_bits(0x3FF4, 0xBB5D54E369F76EDE), // 7.147391378143610789273E-4L
    L::from_bits(0xBFEF, 0xC64B44430295C395), // -2.363848809501759061727E-5L
    L::from_bits(0xBFF4, 0x9BFB5E477C59BA6F), // -5.950237554056330156018E-4L
    L::from_bits(0x3FF1, 0x9293B11D1A395704), // 6.989332260623193171870E-5L
    L::from_bits(0x3FF4, 0xCD8798B21A2130B7), // 7.840334842744753003862E-4L
    L::from_bits(0xBFF2, 0xF09E6A087023BEF3), // -2.294719747873185405699E-4L
    L::from_bits(0xBFF6, 0xAFB934785AC83A1C), // -2.681327161876304418288E-3L
    L::from_bits(0x3FF6, 0xE38E38E3906EC3C9), // 3.472222222230075327854E-3L
    L::from_bits(0x3FFB, 0xAAAAAAAAAAAAA1D5), // 8.333333333333331800504E-2L
];
const TG_SQTPI: L = L::from_bits(0x4000, 0xA06C98FFB1382CB3); // 2.50662827463100050242E0L
const TG_S: [L; 9] = [
    L::from_bits(0xBFF5, 0x9C7E25E5D6D3BAEB), // -1.193945051381510095614E-3L
    L::from_bits(0x3FF7, 0xEC9AC74ECEB4FE9A), // 7.220599478036909672331E-3L
    L::from_bits(0xBFF8, 0x9DA5B0E9DFEF9225), // -9.622023360406271645744E-3L
    L::from_bits(0xBFFA, 0xACD787DCEC1710B0), // -4.219773360705915470089E-2L
    L::from_bits(0x3FFC, 0xAA89190575156B8D), // 1.665386113720805206758E-1L
    L::from_bits(0xBFFA, 0xAC0AF47D126BF183), // -4.200263503403344054473E-2L
    L::from_bits(0xBFFE, 0xA7E7A01357D17BF6), // -6.558780715202540684668E-1L
    L::from_bits(0x3FFE, 0x93C467E37DB0C7A9), // 5.772156649015328608253E-1L
    L::from_bits(0x3FFF, 0x8000000000000000), // 1.000000000000000000000E0L
];
const TG_SN: [L; 9] = [
    L::from_bits(0x3FF5, 0x948DB9F702DE5DD1), // 1.133374167243894382010E-3L
    L::from_bits(0x3FF7, 0xEC9CC5F1DD68989B), // 7.220837261893170325704E-3L
    L::from_bits(0x3FF8, 0x9DA5386F18F02CA1), // 9.621911155035976733706E-3L
    L::from_bits(0xBFFA, 0xACD787D141DD783F), // -4.219773343731191721664E-2L
    L::from_bits(0xBFFC, 0xAA891905D76D7A5B), // -1.665386113944413519335E-1L
    L::from_bits(0xBFFA, 0xAC0AF47D12347F64), // -4.200263503402112910504E-2L
    L::from_bits(0x3FFE, 0xA7E7A01357D15E26), // 6.558780715202536547116E-1L
    L::from_bits(0x3FFE, 0x93C467E37DB0C7AA), // 5.772156649015328608727E-1L
    L::from_bits(0xBFFF, 0x8000000000000000), // -1.000000000000000000000E0L
];
const TG_PIL: L = L::from_bits(0x4000, 0xC90FDAA22168C235); // 3.1415926535897932384626L
const TG_ST0: L = L::from_bits(0x3FF1, 0x923B_0241_CE01_C3F2);
const TG_ST1: L = L::from_bits(0x3FF4, 0xCD87_FB43_A796_20E4);
const TG_ST2: L = L::from_bits(0xBFF2, 0xF09E_7232_FD42_CAB1);
const TG_ST3: L = L::from_bits(0xBFF6, 0xAFB9_3476_D5A6_3DF2);
const TG_ST4: L = L::from_bits(0x3FF6, 0xE38E_38E3_8E38_E38E);
const TG_ST5: L = L::from_bits(0x3FFB, 0xAAAA_AAAA_AAAA_AAAB);
/// Beyond it gamma(x) overflows (musl's `MAXGAML`, 1755.455).
const TG_MAXGAML: L = L::from_bits(0x4009, 0xDB6E_8F5C_28F5_C28F);

/// Gamma by Stirling's formula (musl's `stirf`).
fn stirf(x: L) -> L {
    let mut w = ONE / x;
    if x > ld(1024.0) {
        // For large x, the analytical expansion's rational coefficients.
        w = (((((TG_ST0 * w + TG_ST1) * w + TG_ST2) * w + TG_ST3) * w + TG_ST4) * w + TG_ST5) * w
            + ONE;
    } else {
        w = ONE + w * polevl(w, &TG_STIR);
    }
    let mut y = expl_raw(x);
    if x > ld(1024.0) {
        // Avoid overflow in pow().
        let v = powl_raw(x, ld(0.5) * x - ld(0.25));
        y = v * (v / y);
    } else {
        y = powl_raw(x, x - ld(0.5)) / y;
    }
    TG_SQTPI * y * w
}

/// The gamma function (musl's ld80 `tgammal`), computed to nearest -- in
/// the directed modes its products and quotients lost up to 7 ulps.
///
/// glibc's `w_tgamma_template.c`: a non-finite or zero answer from a finite
/// argument (or -inf) is `ERANGE` at 0 and `EDOM` at a negative integer --
/// the argument's doing, reported here -- and `ERANGE` otherwise, an
/// overflow or an underflow ([`ranged`]).
#[must_use]
pub fn tgammal(x: L) -> L {
    let exact = x.is_zero() || (x < ZERO && floorl(x) == x);
    if x.is_zero() {
        set(errno::ERANGE);
    } else if exact {
        // -inf included.
        set(errno::EDOM);
    }
    ranged(
        x,
        |x| crate::fenv::in_nearest_x87(x, tgammal_raw),
        |y| overflow_or_underflow(x.is_finite() && !exact, y),
    )
}

#[allow(clippy::too_many_lines)]
fn tgammal_raw(x: L) -> L {
    if !x.is_finite() {
        return x + L::INFINITY;
    }
    let q = x.abs();
    if q > ld(13.0) {
        let z = if x < ZERO {
            let mut p = floorl(q);
            // gamma(x) is negative for x in (-n-1, -n) with n even: the
            // parity of floor(|x|), taken before p is rounded up below.
            // musl tests p after that rounding, so for |x| > 13 with a
            // fraction above one half it answers the wrong sign --
            // tgammal(-22.96) positive; Cephes, where the code comes from,
            // takes the parity first, as this does.
            let floor_q = p;
            let mut z = q - p;
            if z.is_zero() {
                return ZERO / z;
            }
            if q > TG_MAXGAML {
                z = ZERO;
            } else {
                if z > ld(0.5) {
                    p = p + ONE;
                    z = q - p;
                }
                z = q * sinl(TG_PIL * z);
                z = z.abs() * stirf(q);
                z = TG_PIL / z;
            }
            if ld(0.5) * floor_q == floorl(q * ld(0.5)) {
                z = -z;
            }
            z
        } else if x > TG_MAXGAML {
            x * L::from_bits(0x3FFF + 16383, 1 << 63)
        } else {
            stirf(x)
        };
        return z;
    }
    let mut x = x;
    let mut z = ONE;
    while x >= ld(3.0) {
        x = x - ONE;
        z = z * x;
    }
    let small = ld(0.031_25);
    while x < -small {
        z = z / x;
        x = x + ONE;
    }
    if x > small {
        while x < ld(2.0) {
            z = z / x;
            x = x + ONE;
        }
        if x == ld(2.0) {
            return z;
        }
        x = x - ld(2.0);
        let p = polevl(x, &TG_P);
        let q = polevl(x, &TG_Q);
        return z * p / q;
    }
    // Small: z == 1 if x was originally +-0.
    if x.is_zero() && z != ONE {
        return x / x;
    }
    if x < ZERO {
        let x = -x;
        z / (x * polevl(x, &TG_SN))
    } else {
        z / (x * polevl(x, &TG_S))
    }
}

// ===========================================================================
// Fused multiply-add: FreeBSD's s_fmal.c, as musl has it
// ===========================================================================

/// `a + b` with the last bit of the result made a sticky bit for whatever
/// the addition lost, so that a later addition to something larger rounds
/// once, correctly (FreeBSD's `add_adjusted`).
fn add_adjusted(a: L, b: L) -> L {
    let (hi, lo) = two_sum(a, b);
    if !lo.is_zero() && hi.significand & 1 == 0 {
        return next_raw(hi, lo > ZERO);
    }
    hi
}

/// `ldexp(a + b, scale)` rounded once, for a result known to be subnormal
/// (FreeBSD's `add_and_denormalize`). The number of bits the scaling drops
/// is computed from the biased exponent alone, as FreeBSD does; musl reads
/// the sign with it, which a negative sum gets wrong.
fn add_and_denormalize(a: L, b: L, scale: i32) -> L {
    let (mut hi, lo) = two_sum(a, b);
    if !lo.is_zero() {
        let bits_lost = -i32::from(hi.biased_exponent()) - scale + 1;
        if (bits_lost != 1) ^ (hi.significand & 1 != 0) {
            hi = next_raw(hi, lo > ZERO);
        }
    }
    scalbnl_raw(hi, scale)
}

/// `x * y + z` rounded once (FreeBSD's `fmal`): the product exactly as two
/// long doubles by Dekker's method, the sum with `z` exactly, and one
/// rounding at the end, with the low bit of the correction made sticky.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn fmal(x: L, y: L, z: L) -> L {
    use crate::fenv::{
        FE_DOWNWARD, FE_INEXACT, FE_TONEAREST, FE_TOWARDZERO, FE_UNDERFLOW, FE_UPWARD,
    };
    // The order of these, and what each returns, is what makes the
    // infinities, NaNs, overflows and signed zeros come out right.
    if !x.is_finite() || !y.is_finite() {
        return x * y + z;
    }
    if !z.is_finite() {
        return z;
    }
    if x.is_zero() || y.is_zero() {
        return x * y + z;
    }
    if z.is_zero() {
        return x * y;
    }
    let (xs, ex) = frexpl(x);
    let (ys, ey) = frexpl(y);
    let (zs, ez) = frexpl(z);
    let oround = crate::fenv::fegetround();
    let spread = ex + ey - ez;
    // x*y and z many orders of magnitude apart: z, nudged by the rounding
    // mode, with the flags the true sum would raise.
    if spread < -64 {
        crate::fenv::feraiseexcept(FE_INEXACT);
        if z.biased_exponent() == 0 {
            crate::fenv::feraiseexcept(FE_UNDERFLOW);
        }
        let xy_neg = (x > ZERO) ^ (y < ZERO);
        return match oround {
            FE_TOWARDZERO => {
                if xy_neg ^ (z < ZERO) {
                    z
                } else {
                    next_raw(z, z.is_sign_negative())
                }
            }
            FE_DOWNWARD => {
                if xy_neg {
                    z
                } else {
                    next_raw(z, false)
                }
            }
            FE_UPWARD => {
                if xy_neg {
                    next_raw(z, true)
                } else {
                    z
                }
            }
            _ => z,
        };
    }
    let zs = if spread <= 64 * 2 {
        scalbnl_raw(zs, -spread)
    } else {
        L::from_bits(0x0001, 1 << 63).copysign(zs) // LDBL_MIN
    };
    crate::fenv::fesetround(FE_TONEAREST);
    // (xy.hi, xy.lo) = x * y (exact); (r.hi, r.lo) = xy.hi + z (exact);
    // adj = xy.lo + r.lo (inexact, low bit sticky); r.hi + adj rounds once.
    let (xy_hi, xy_lo) = two_prod(xs, ys);
    let (r_hi, r_lo) = two_sum(xy_hi, zs);
    let spread = ex + ey;
    if r_hi.is_zero() {
        // The addends cancelled to 0: make sure of the sign.
        crate::fenv::fesetround(oround);
        let vzs = core::hint::black_box(zs);
        return xy_hi + vzs + scalbnl_raw(xy_lo, spread);
    }
    if oround != FE_TONEAREST {
        // No double rounding to worry about in a directed mode, but the
        // underflow flag has to be raised by hand.
        let had_inexact = crate::fenv::fetestexcept(FE_INEXACT);
        crate::fenv::feclearexcept(FE_INEXACT);
        crate::fenv::fesetround(oround);
        let adj = r_lo + xy_lo;
        let ret = scalbnl_raw(r_hi + adj, spread);
        if ilogbl_raw(ret) < -16382 && crate::fenv::fetestexcept(FE_INEXACT) != 0 {
            crate::fenv::feraiseexcept(FE_UNDERFLOW);
        } else if had_inexact != 0 {
            crate::fenv::feraiseexcept(FE_INEXACT);
        }
        return ret;
    }
    let adj = add_adjusted(r_lo, xy_lo);
    if spread + ilogbl_raw(r_hi) > -16383 {
        scalbnl_raw(r_hi + adj, spread)
    } else {
        add_and_denormalize(r_hi, adj, spread)
    }
}

// ===========================================================================
// Sine, cosine, tangent: musl's ld80 `__rem_pio2l`, `__sinl`, `__cosl`,
// `__tanl` and their callers
// ===========================================================================

/// `1.5/LDBL_EPSILON`: adding and subtracting it rounds to an integer, from
/// either side of zero (musl's `toint` in `__rem_pio2l`).
const TOINT_15: L = L::from_bits(0x3FFF + 63, 0xC000_0000_0000_0000);
/// 64 bits of 2/pi (`0xa2f9836e4e44152a.0p-64`).
const INVPIO2: L = L::from_bits(0x3FFE, 0xA2F9_836E_4E44_152A);
/// pi/4 (`0xc90fdaa22168c235.0p-64`).
const PIO4: L = L::from_bits(0x3FFE, 0xC90F_DAA2_2168_C235);
/// pi/2 in three 39-bit doubles, and what each leaves of pi/2 as a long
/// double (musl's `pio2_1`..`pio2_3t`).
const PIO2_1: f64 = f64::from_bits(0x3FF9_21FB_5444_4000);
const PIO2_2: f64 = f64::from_bits(0xBD72_E7B9_6767_4000);
const PIO2_3: f64 = f64::from_bits(0x3AE8_A2E0_3707_4000);
const PIO2_1T: L = L::from_bits(0xBFD7, 0x973D_CB3B_399D_747F);
const PIO2_2T: L = L::from_bits(0x3FAE, 0xC517_01B8_39A2_5205);
const PIO2_3T: L = L::from_bits(0xBF85, 0xBB5B_F6C7_DDD6_60CE);

/// `x` reduced by pi/2: `n` and the remainder `y0 + y1`, `|y0 + y1| <=
/// pi/4` (musl's `__rem_pio2l`). Cody and Waite's subtraction of pi/2 in up
/// to three pieces for `|x|` below about `2^25 pi/2`; Payne and Hanek's
/// ([`crate::rem_pio2_large`]) above.
fn rem_pio2l(x: L) -> (i32, L, L) {
    let ex = i32::from(x.biased_exponent());
    let key = (u32::from(x.sign_exp & 0x7FFF) << 16) | ((x.significand >> 48) as u32);
    if key < (((0x3FFF + 25) << 16) | (0x921F >> 1) | 0x8000) {
        // rint(x / (pi/2))
        let mut fn_ = x * INVPIO2 + TOINT_15 - TOINT_15;
        #[allow(clippy::cast_possible_truncation)]
        let mut n = (fn_.to_i64_rint() as i32) & 0x7FFF_FFFF;
        let mut r = x - fn_ * ld(PIO2_1);
        let mut w = fn_ * PIO2_1T;
        // Only a directed rounding mode can leave the first guess off by one.
        if r - w < -PIO4 {
            n = n.wrapping_sub(1);
            fn_ = fn_ - ONE;
            r = x - fn_ * ld(PIO2_1);
            w = fn_ * PIO2_1T;
        } else if r - w > PIO4 {
            n = n.wrapping_add(1);
            fn_ = fn_ + ONE;
            r = x - fn_ * ld(PIO2_1);
            w = fn_ * PIO2_1T;
        }
        let mut y0 = r - w;
        let ey = i32::from(y0.biased_exponent());
        if ex - ey > 22 {
            // A second iteration, good to 141 bits.
            let t = r;
            w = fn_ * ld(PIO2_2);
            r = t - w;
            w = fn_ * PIO2_2T - ((t - r) - w);
            y0 = r - w;
            let ey = i32::from(y0.biased_exponent());
            if ex - ey > 61 {
                // A third, good to 180 bits: enough for every case.
                let t = r;
                w = fn_ * ld(PIO2_3);
                r = t - w;
                w = fn_ * PIO2_3T - ((t - r) - w);
                y0 = r - w;
            }
        }
        let y1 = (r - y0) - w;
        return (n, y0, y1);
    }
    if ex == 0x7FFF {
        let nan = x - x;
        return (0, nan, nan);
    }
    // z = |x| scaled to [2^23, 2^24), cut into three 24-bit doubles: the
    // integer part, then the next 24 bits twice (64 bits in all).
    let mut z = L::from_bits(0x3FFF + 23, x.significand);
    let mut take24 = || {
        #[allow(clippy::cast_possible_truncation)]
        let whole = f64::from(z.to_f64() as i32);
        z = (z - ld(whole)) * ld(16_777_216.0);
        whole
    };
    let t0 = take24();
    let t1 = take24();
    let t2 = z.to_f64();
    let tx = [t0, t1, t2];
    // Trailing zero pieces are left out, as musl's `while (tx[i] == 0) i--`.
    let nx = if t2 != 0.0 {
        3
    } else if t1 != 0.0 {
        2
    } else {
        1
    };
    let mut ty = [0.0_f64; 3];
    let n = crate::rem_pio2_large::rem_pio2_large(
        tx.get(..nx).unwrap_or(&tx),
        &mut ty,
        ex - 0x3FFF - 23,
        2,
    );
    let [ty0, ty1, _] = ty;
    let mut w = ld(ty1);
    let r = ld(ty0) + w;
    w = w - (r - ld(ty0));
    if x.is_sign_negative() {
        return (-n, -r, -w);
    }
    (n, r, w)
}

/// `S2..S8` of musl's ld80 `__sinl`: `|sin(x)/x - s(x)| < 2^-72.1` on
/// `[-pi/4, pi/4]`.
const S1: L = L::from_bits(0xBFFC, 0xAAAA_AAAA_AAAA_AAAB);
const S: [f64; 7] = [
    f64::from_bits(0x3F81_1111_1111_1111),
    f64::from_bits(0xBF2A_01A0_1A01_9F81),
    f64::from_bits(0x3EC7_1DE3_A555_60F7),
    f64::from_bits(0xBE5A_E645_64F1_6CAD),
    f64::from_bits(0x3DE6_1242_B902_43B5),
    f64::from_bits(0xBD6A_E42E_BD1B_2E00),
    f64::from_bits(0x3CE7_9372_EA0B_3F64),
];

/// Horner's rule over double coefficients, in long double.
fn horner(z: L, c: &[f64]) -> L {
    let mut acc = ZERO;
    let mut first = true;
    for &k in c.iter().rev() {
        acc = if first { ld(k) } else { ld(k) + z * acc };
        first = false;
    }
    acc
}

/// The sine of `x + y`, `|x + y| <= pi/4` (musl's `__sinl`); `iy == 0` when
/// `y` is known to be zero.
fn k_sinl(x: L, y: L, iy: bool) -> L {
    let z = x * x;
    let v = z * x;
    let r = horner(z, &S);
    if !iy {
        return x + v * (S1 + z * r);
    }
    x - ((z * (ld(0.5) * y - v * r) - y) - v * S1)
}

const C1: L = L::from_bits(0x3FFA, 0xAAAA_AAAA_AAAA_AA9B);
const C: [f64; 6] = [
    f64::from_bits(0xBF56_C16C_16C1_6C10),
    f64::from_bits(0x3EFA_01A0_1A01_8E22),
    f64::from_bits(0xBE92_7E4F_B760_2F22),
    f64::from_bits(0x3E21_EED8_CAAE_CCF1),
    f64::from_bits(0xBDA9_3934_12BD_1529),
    f64::from_bits(0x3D2A_AC9D_9AF5_C43E),
];

/// The cosine of `x + y`, `|x + y| <= pi/4` (musl's `__cosl`).
fn k_cosl(x: L, y: L) -> L {
    let z = x * x;
    let r = z * (C1 + z * horner(z, &C));
    let hz = ld(0.5) * z;
    let w = ONE - hz;
    w + (((ONE - w) - hz) + (z * r - x * y))
}

const T3: L = L::from_bits(0x3FFD, 0xAAAA_AAAA_AAAA_AAA5);
const T5: L = L::from_bits(0x3FFC, 0x8888_8888_8888_93C3);
const T7: L = L::from_bits(0x3FFA, 0xDD0D_D0DD_0DC1_3BA2);
const PIO4LO: L = L::from_bits(0xBFBC, 0xECE6_75D1_FC8F_8CBB);
/// `T9, T13, .. T33` and `T11, T15, .. T31` of musl's ld80 `__tanl`.
const TR: [f64; 7] = [
    f64::from_bits(0x3F96_64F4_882C_C1C2),
    f64::from_bits(0x3F6D_6D3D_185D_7FF8),
    f64::from_bits(0x3F43_5593_5868_5B83),
    f64::from_bits(0x3F19_77EF_C268_06F4),
    f64::from_bits(0x3EF2_F5E5_63E5_487E),
    f64::from_bits(0x3EE0_6B59_141A_6CB3),
    f64::from_bits(0x3EC3_8354_36C0_C87F),
];
const TV: [f64; 6] = [
    f64::from_bits(0x3F82_26E3_55C1_7612),
    f64::from_bits(0x3F57_DA35_4AA3_F96B),
    f64::from_bits(0x3F2F_5624_2026_B5BE),
    f64::from_bits(0x3F04_275A_09B3_CEAC),
    f64::from_bits(0x3EC4_4C0D_80CC_6896),
    f64::from_bits(0xBECB_5ABE_F3BA_4B59),
];

/// The tangent of `x + y` (`odd == false`) or `-1/tan(x + y)` (`odd`),
/// `|x + y| <= pi/4` (musl's `__tanl`).
fn k_tanl(x: L, y: L, odd: bool) -> L {
    let mut x = x;
    let mut y = y;
    let big = x.abs() >= ld(0.674_34);
    let mut neg = false;
    if big {
        if x < ZERO {
            neg = true;
            x = -x;
            y = -y;
        }
        x = (PIO4 - x) + (PIO4LO - y);
        y = ZERO;
    }
    let z = x * x;
    let w = z * z;
    let r = T5 + w * horner(w, &TR);
    let v = z * (T7 + w * horner(w, &TV));
    let s = z * x;
    let r = y + z * (s * (r + v) + y) + T3 * s;
    let w = x + r;
    if big {
        let s = if odd { -ONE } else { ONE };
        let v = s - ld(2.0) * (x + (r - w * w / (w + s)));
        return if neg { -v } else { v };
    }
    if !odd {
        return w;
    }
    // -1/(x + r), computed accurately.
    let two32 = ld(4_294_967_296.0);
    let z = w + two32 - two32;
    let v = r - (z - x);
    let a = -ONE / w;
    let t = a + two32 - two32;
    let s = ONE + t * z;
    t + a * (s + t * v)
}

/// glibc's `s_sinl.c`, `s_cosl.c`, `s_tanl.c`, `s_sincosl.c`: an infinite
/// argument is a domain error.
fn trig_domain(x: L) {
    if x.is_infinite() {
        set(errno::EDOM);
    }
}

/// `|x| < 2^-32`: `sin(x)` and `tan(x)` are `x` to the last bit. Raise
/// inexact for a nonzero `x`, and underflow for a subnormal one.
fn tiny_trig(x: L, biased: u16) -> L {
    if biased == 0 {
        force_eval(x * L::from_bits(0x3FFF - 120, 1 << 63));
    } else {
        force_eval(x + L::from_bits(0x3FFF + 120, 1 << 63));
    }
    x
}

/// The sine, computed to nearest ([`rem_pio2l`]'s last pieces are exact
/// only there).
#[must_use]
pub fn sinl(x: L) -> L {
    trig_domain(x);
    crate::fenv::in_nearest_x87(x, sinl_near)
}

fn sinl_near(x: L) -> L {
    let e = x.biased_exponent();
    if e == 0x7FFF {
        return x - x;
    }
    if x.abs() < ld(core::f64::consts::FRAC_PI_4) {
        if e < 0x3FFF - 32 {
            return tiny_trig(x, e);
        }
        return k_sinl(x, ZERO, false);
    }
    let (n, hi, lo) = rem_pio2l(x);
    match n & 3 {
        0 => k_sinl(hi, lo, true),
        1 => k_cosl(hi, lo),
        2 => -k_sinl(hi, lo, true),
        _ => -k_cosl(hi, lo),
    }
}

/// The cosine, computed to nearest.
#[must_use]
pub fn cosl(x: L) -> L {
    trig_domain(x);
    crate::fenv::in_nearest_x87(x, cosl_near)
}

fn cosl_near(x: L) -> L {
    let e = x.biased_exponent();
    if e == 0x7FFF {
        return x - x;
    }
    let a = x.abs();
    if a < ld(core::f64::consts::FRAC_PI_4) {
        if e < 0x3FFF - 64 {
            // Raise inexact if x != 0.
            return ONE + a;
        }
        return k_cosl(a, ZERO);
    }
    let (n, hi, lo) = rem_pio2l(a);
    match n & 3 {
        0 => k_cosl(hi, lo),
        1 => -k_sinl(hi, lo, true),
        2 => -k_cosl(hi, lo),
        _ => k_sinl(hi, lo, true),
    }
}

/// The tangent, computed to nearest.
#[must_use]
pub fn tanl(x: L) -> L {
    trig_domain(x);
    crate::fenv::in_nearest_x87(x, tanl_near)
}

fn tanl_near(x: L) -> L {
    let e = x.biased_exponent();
    if e == 0x7FFF {
        return x - x;
    }
    if x.abs() < ld(core::f64::consts::FRAC_PI_4) {
        if e < 0x3FFF - 32 {
            return tiny_trig(x, e);
        }
        return k_tanl(x, ZERO, false);
    }
    let (n, hi, lo) = rem_pio2l(x);
    k_tanl(hi, lo, n & 1 != 0)
}

/// The sine and the cosine at once, computed to nearest.
#[must_use]
pub fn sincosl(x: L) -> (L, L) {
    trig_domain(x);
    crate::fenv::in_nearest_x87(x, sincosl_near)
}

fn sincosl_near(x: L) -> (L, L) {
    let e = x.biased_exponent();
    if e == 0x7FFF {
        let nan = x - x;
        return (nan, nan);
    }
    if x.abs() < ld(core::f64::consts::FRAC_PI_4) {
        if e < 0x3FFF - 64 {
            if e == 0 {
                force_eval(x * L::from_bits(0x3FFF - 120, 1 << 63));
            }
            return (x, ONE + x);
        }
        return (k_sinl(x, ZERO, false), k_cosl(x, ZERO));
    }
    let (n, hi, lo) = rem_pio2l(x);
    let s = k_sinl(hi, lo, true);
    let c = k_cosl(hi, lo);
    match n & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

// ===========================================================================
// The C entry points
// ===========================================================================
//
// Each C function is an assembly thunk (`crate::ld_c!`, in `ld_abi.rs`) into
// the `extern "C"` shim below it, which reads its long-double arguments
// through pointers and writes a long-double result through one; the shim
// calls the safe function of the C name above. Only the bare-metal build
// has them: the host's C library owns these names there.

/// Shims and thunks for `long double f(long double)`.
macro_rules! export_l_l {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, out: *mut L) {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument and the thunk's result slot, both long-double slots.
            unsafe { out.write($f(x.read())) }
        }
        crate::ld_c!(l_l $c => $shim);
    )* };
}

/// Shims and thunks for `long double f(long double, long double)`.
macro_rules! export_l_ll {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, y: *const L, out: *mut L) {
            // SAFETY: as in `export_l_l!`.
            unsafe { out.write($f(x.read(), y.read())) }
        }
        crate::ld_c!(l_ll $c => $shim);
    )* };
}

/// Shims and thunks for `int f(long double)` and `long f(long double)`.
macro_rules! export_i_l {
    ($($c:literal $shim:ident $f:path, $ty:ty;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L) -> $ty {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument.
            $f(unsafe { x.read() })
        }
        crate::ld_c!(i_l $c => $shim);
    )* };
}

export_l_l! {
    "acoshl" __slate_ld_acoshl acoshl;
    "acosl" __slate_ld_acosl acosl;
    "asinhl" __slate_ld_asinhl asinhl;
    "asinl" __slate_ld_asinl asinl;
    "atanhl" __slate_ld_atanhl atanhl;
    "atanl" __slate_ld_atanl atanl;
    "cbrtl" __slate_ld_cbrtl cbrtl;
    "ceill" __slate_ld_ceill ceill;
    "coshl" __slate_ld_coshl coshl;
    "cosl" __slate_ld_cosl cosl;
    "erfcl" __slate_ld_erfcl erfcl;
    "erfl" __slate_ld_erfl erfl;
    "exp10l" __slate_ld_exp10l exp10l;
    "exp2l" __slate_ld_exp2l exp2l;
    "expl" __slate_ld_expl expl;
    "expm1l" __slate_ld_expm1l expm1l;
    "fabsl" __slate_ld_fabsl fabsl;
    "floorl" __slate_ld_floorl floorl;
    "gammal" __slate_ld_gammal lgammal;
    "lgammal" __slate_ld_lgammal lgammal;
    "log10l" __slate_ld_log10l log10l;
    "log1pl" __slate_ld_log1pl log1pl;
    "log2l" __slate_ld_log2l log2l;
    "logbl" __slate_ld_logbl logbl;
    "logl" __slate_ld_logl logl;
    "nearbyintl" __slate_ld_nearbyintl nearbyintl;
    "rintl" __slate_ld_rintl rintl;
    "pow10l" __slate_ld_pow10l exp10l;
    "roundevenl" __slate_ld_roundevenl roundevenl;
    "roundl" __slate_ld_roundl roundl;
    "significandl" __slate_ld_significandl significandl;
    "sinhl" __slate_ld_sinhl sinhl;
    "sinl" __slate_ld_sinl sinl;
    "sqrtl" __slate_ld_sqrtl sqrtl;
    "tanhl" __slate_ld_tanhl tanhl;
    "tanl" __slate_ld_tanl tanl;
    "tgammal" __slate_ld_tgammal tgammal;
    "truncl" __slate_ld_truncl truncl;
}

export_l_ll! {
    "atan2l" __slate_ld_atan2l atan2l;
    "copysignl" __slate_ld_copysignl copysignl;
    "dreml" __slate_ld_dreml remainderl;
    "fdiml" __slate_ld_fdiml fdiml;
    "fmaxl" __slate_ld_fmaxl fmaxl;
    "fminl" __slate_ld_fminl fminl;
    "fmodl" __slate_ld_fmodl fmodl;
    "hypotl" __slate_ld_hypotl hypotl;
    "nextafterl" __slate_ld_nextafterl nextafterl;
    "nexttowardl" __slate_ld_nexttowardl nexttowardl;
    "powl" __slate_ld_powl powl;
    "remainderl" __slate_ld_remainderl remainderl;
}

export_i_l! {
    "ilogbl" __slate_ld_ilogbl ilogbl, i32;
    "lrintl" __slate_ld_lrintl lrintl, i64;
    "llrintl" __slate_ld_llrintl llrintl, i64;
    "lroundl" __slate_ld_lroundl lroundl, i64;
    "llroundl" __slate_ld_llroundl llroundl, i64;
    "__fpclassifyl" __slate_ld_fpclassifyl fpclassifyl, i32;
    "__signbitl" __slate_ld_signbitl signbitl, i32;
    "finitel" __slate_ld_finitel finitel, i32;
    "isinfl" __slate_ld_isinfl isinfl, i32;
    "isnanl" __slate_ld_isnanl isnanl, i32;
}

/// `x * y + z` (`fmal`).
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_fmal(x: *const L, y: *const L, z: *const L, out: *mut L) {
    // SAFETY: as in `export_l_l!`.
    unsafe { out.write(fmal(x.read(), y.read(), z.read())) }
}
crate::ld_c!(l_lll "fmal" => __slate_ld_fmal);

/// `x * 2^n` (`ldexpl`, `scalbnl`).
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_scalbnl(x: *const L, n: i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`.
    unsafe { out.write(scalbnl(x.read(), n)) }
}
crate::ld_c!(l_li "scalbnl" => __slate_ld_scalbnl);
crate::ld_c!(l_li "ldexpl" => __slate_ld_scalbnl);

/// `x * 2^n` with a `long` exponent.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_scalblnl(x: *const L, n: i64, out: *mut L) {
    // SAFETY: as in `export_l_l!`.
    unsafe { out.write(scalblnl(x.read(), n)) }
}
crate::ld_c!(l_ln "scalblnl" => __slate_ld_scalblnl);

/// `frexpl(x, &e)`: a NULL `e` is not written.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_frexpl(x: *const L, e: *mut i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`; `e` is the caller's `int *`, checked.
    unsafe {
        let (m, ex) = frexpl(x.read());
        if !e.is_null() {
            e.write(ex);
        }
        out.write(m);
    }
}
crate::ld_c!(l_lp "frexpl" => __slate_ld_frexpl);

/// `modfl(x, &i)`: a NULL `i` is not written.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_modfl(x: *const L, i: *mut L, out: *mut L) {
    // SAFETY: as in `export_l_l!`; `i` is the caller's `long double *`,
    // checked.
    unsafe {
        let (f, ip) = modfl(x.read());
        if !i.is_null() {
            i.write(ip);
        }
        out.write(f);
    }
}
crate::ld_c!(l_lp "modfl" => __slate_ld_modfl);

/// `lgammal_r(x, &sign)`: a NULL `sign` is not written.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_lgammal_r(x: *const L, sign: *mut i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`; `sign` is the caller's `int *`, checked.
    unsafe {
        let (y, s) = lgammal_r(x.read());
        if !sign.is_null() {
            sign.write(s);
        }
        out.write(y);
    }
}
crate::ld_c!(l_lp "lgammal_r" => __slate_ld_lgammal_r);

/// `remquol(x, y, &q)`: a NULL `q` is not written.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_remquol(x: *const L, y: *const L, q: *mut i32, out: *mut L) {
    // SAFETY: as in `export_l_l!`; `q` is the caller's `int *`, checked.
    unsafe {
        let (r, quo) = remquol(x.read(), y.read());
        if !q.is_null() {
            q.write(quo);
        }
        out.write(r);
    }
}
crate::ld_c!(l_llp "remquol" => __slate_ld_remquol);

/// `nanl(tag)`.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_nanl(tag: *const u8, out: *mut L) {
    // SAFETY: `tag` is NULL or the caller's C string (`nanl`'s contract);
    // `out` is the thunk's result slot.
    unsafe { out.write(nanl(tag)) }
}
crate::ld_c!(l_p "nanl" => __slate_ld_nanl);

/// `sincosl(x, &s, &c)`: a NULL pointer is not written.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_sincosl(x: *const L, s: *mut L, c: *mut L) {
    // SAFETY: as in `export_l_l!`; `s` and `c` are the caller's
    // `long double *`s, checked.
    unsafe {
        let (sn, cs) = sincosl(x.read());
        if !s.is_null() {
            s.write(sn);
        }
        if !c.is_null() {
            c.write(cs);
        }
    }
}
crate::ld_c!(v_lpp "sincosl" => __slate_ld_sincosl);

/// `nexttoward(x, y)`: a double toward a long double.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_nexttoward(y: *const L, x: f64) -> f64 {
    // SAFETY: called only by its thunk, with the caller's stack argument.
    nexttoward(x, unsafe { y.read() })
}
crate::ld_c!(d_dl "nexttoward" => __slate_ld_nexttoward);

/// `nexttowardf(x, y)`: a float toward a long double.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_nexttowardf(y: *const L, x: f32) -> f32 {
    // SAFETY: as for `nexttoward`.
    nexttowardf(x, unsafe { y.read() })
}
crate::ld_c!(f_fl "nexttowardf" => __slate_ld_nexttowardf);

// ===========================================================================
// Classification, and glibc's other names
// ===========================================================================

/// `__fpclassifyl`, which musl's `fpclassify` (and `isnan`, `isinf`,
/// `isfinite`, `isnormal`) call for a long double: `FP_NAN`, `FP_INFINITE`,
/// `FP_ZERO`, `FP_SUBNORMAL` or `FP_NORMAL`, as `math.rs` numbers them. A
/// pseudo-denormal (exponent 0, integer bit set) is `FP_NORMAL`, and an
/// encoding the unit rejects `FP_NAN`, as musl's `__fpclassifyl` says.
#[must_use]
pub fn fpclassifyl(x: L) -> i32 {
    let e = x.biased_exponent();
    let msb = x.significand >> 63 != 0;
    if e == 0 {
        if msb {
            return crate::math::FP_NORMAL;
        }
        return if x.significand == 0 {
            crate::math::FP_ZERO
        } else {
            crate::math::FP_SUBNORMAL
        };
    }
    if !msb {
        return crate::math::FP_NAN;
    }
    if e == 0x7FFF {
        return if x.significand << 1 != 0 {
            crate::math::FP_NAN
        } else {
            crate::math::FP_INFINITE
        };
    }
    crate::math::FP_NORMAL
}

/// `__signbitl`: 512 when the sign bit is set, else 0. C promises only
/// "nonzero"; 512 is glibc's value (its x86-64 version is `fxam`, reading
/// the sign from the status word's C1, bit 9), kept so that a program
/// printing the result prints what it prints on glibc.
#[must_use]
pub fn signbitl(x: L) -> i32 {
    if x.is_sign_negative() { 0x200 } else { 0 }
}

/// BSD's `finitel`: 1 unless the exponent is all ones. glibc looks at the
/// exponent alone, and so does this, so an unnormal -- which the unit loads
/// as a NaN, and `isfinite` calls one -- is "finite" here, as on glibc.
#[must_use]
pub fn finitel(x: L) -> i32 {
    i32::from(x.biased_exponent() != 0x7FFF)
}

/// BSD's `isinfl`: 1 for +inf, -1 for -inf, else 0 (glibc's values; a
/// pseudo-infinity, integer bit clear, is no infinity).
#[must_use]
pub fn isinfl(x: L) -> i32 {
    if x.is_infinite() {
        if x.is_sign_negative() { -1 } else { 1 }
    } else {
        0
    }
}

/// BSD's `isnanl`, glibc's x86 `s_isnanl.c` operation for operation: 65535
/// for a NaN (the all-ones exponent with a fraction bit set), 1 for any
/// other encoding the unit rejects (a nonzero exponent with the integer bit
/// clear), 0 for everything else. The 65535 is the branch-free code's
/// accident -- C promises only "nonzero" -- and is kept for the reason
/// [`signbitl`]'s 512 is.
#[must_use]
#[allow(clippy::cast_possible_truncation)] // glibc's two 32-bit words
pub fn isnanl(x: L) -> i32 {
    let se = u32::from(x.biased_exponent()) << 1;
    let hx = (x.significand >> 32) as u32;
    let lx = x.significand as u32 | (hx & 0x7FFF_FFFF);
    // A pseudo-normal: exponent nonzero, integer bit clear.
    let pn = (!hx & 0x8000_0000 & (se | se.wrapping_neg())) >> 31;
    let se = 0xFFFE_u32.wrapping_sub(se | (lx | lx.wrapping_neg()) >> 31);
    // At most 0xFFFF.
    i32::try_from((se >> 16) | pn).unwrap_or(1)
}

/// glibc's `significandl` on x86-64, which is the `fxtract` instruction:
/// `x` scaled into `[1, 2)`, keeping its sign. A zero or an infinity is its
/// own answer, a zero raising divide-by-zero; a NaN comes back quieted and
/// an encoding the unit rejects as the default NaN. `errno` is never set --
/// unlike `significand`, whose glibc version calls the public `ilogb`,
/// which sets `EDOM`.
#[must_use]
pub fn significandl(x: L) -> L {
    x.fxtract().0
}

/// `nanl(tag)`: a quiet NaN whose payload is `tag` read as `strtoull`
/// reads it, in the 62 bits below the integer and quiet bits (glibc's
/// ldbl-96 `SET_NAN_PAYLOAD`); the default NaN for an empty or unreadable
/// tag.
///
/// # Safety
///
/// `tag` is NULL or a NUL-terminated string.
#[must_use]
pub unsafe fn nanl(tag: *const u8) -> L {
    // SAFETY: this function's contract.
    let payload = crate::math::tag_payload(unsafe { crate::math::tag_bytes(tag) }).unwrap_or(0);
    L::from_bits(
        0x7FFF,
        0xC000_0000_0000_0000 | (payload & 0x3FFF_FFFF_FFFF_FFFF),
    )
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// The x87 precision at 64 bits for this test's thread (the Windows host
    /// starts threads at 53).
    fn extended() {
        let cw: u16 = 0x037F;
        // SAFETY: loads the control word from a local.
        unsafe {
            core::arch::asm!("fldcw [{}]", in(reg) &raw const cw, options(nostack, preserves_flags))
        };
    }

    const ORACLE: &str = include_str!("mathl_oracle.txt");

    /// glibc's answers in the three directed rounding modes, for each call of
    /// [`ORACLE`] that answers otherwise than to nearest: `<mode> <name>
    /// <in...> = <out...> <errno>`, mode `down`, `up` or `zero`
    /// (`posix/tools/oracle/mathl_modes_harness.py`).
    const MODES_ORACLE: &str = include_str!("mathl_modes_oracle.txt");

    std::thread_local! {
        /// What [`ulps`] adds to an inexact function's bound: 1 while
        /// [`every_call_answers_in_every_rounding_direction`] replays a
        /// directed mode; 0 otherwise.
        static DIRECTED_SLACK: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }

    /// Units in the last place allowed, in the 64-bit significand: 0 where
    /// IEEE fixes the answer; for the rest, glibc's documented worst case
    /// (`libm-test-ulps`, x86_64, `ldouble`) plus one for the difference in
    /// algorithms -- and one more in a directed mode ([`DIRECTED_SLACK`]).
    fn ulps(name: &str) -> u64 {
        match ulps_to_nearest(name) {
            0 => 0,
            n => n + DIRECTED_SLACK.with(core::cell::Cell::get),
        }
    }

    /// [`ulps`] to nearest.
    fn ulps_to_nearest(name: &str) -> u64 {
        match name {
            "logl" | "log2l" | "log10l" | "log1pl" | "atanl" | "atan2l" | "asinl" | "acosl"
            | "exp2l" | "expl" | "expm1l" | "sinl" | "cosl" | "tanl" | "sincosl" | "sinhl"
            | "coshl" | "tanhl" | "asinhl" | "acoshl" | "atanhl" | "cbrtl" | "hypotl"
            | "exp10l" => 3,
            "powl" => 3,
            "erfl" | "erfcl" | "lgammal" | "tgammal" => 4,
            _ => 0,
        }
    }

    fn parse_l(t: &str) -> L {
        let (se, m) = t.split_once(':').expect("SSSS:MMMM");
        L::from_bits(
            u16::from_str_radix(se, 16).expect("hex"),
            u64::from_str_radix(m, 16).expect("hex"),
        )
    }

    /// Ours against glibc's: NaN matches NaN; infinities and zeros bit for
    /// bit; the rest within `tol` ulps of the 64-bit significand, same sign.
    fn same(ours: L, glibc: L, tol: u64) -> Result<(), String> {
        if ours.is_nan() || glibc.is_nan() {
            return if ours.is_nan() && glibc.is_nan() {
                Ok(())
            } else {
                Err(format!("{ours:?}, glibc {glibc:?}"))
            };
        }
        let exact = |v: L| v.is_infinite() || v.is_zero();
        if exact(ours) && exact(glibc) || tol == 0 {
            return if (ours.sign_exp, ours.significand) == (glibc.sign_exp, glibc.significand) {
                Ok(())
            } else {
                Err(format!(
                    "{:04x}:{:016x}, glibc {:04x}:{:016x}",
                    ours.sign_exp, ours.significand, glibc.sign_exp, glibc.significand
                ))
            };
        }
        if ours.is_sign_negative() != glibc.is_sign_negative() {
            return Err(format!("{ours:?}, glibc {glibc:?}: the sign"));
        }
        // On one ordered line: the 15-bit exponent and 63 fraction bits.
        let line =
            |v: L| (i128::from(v.biased_exponent()) << 63) | i128::from(v.significand & !(1 << 63));
        let d = (line(ours) - line(glibc)).unsigned_abs();
        if d > u128::from(tol) {
            return Err(format!(
                "{:04x}:{:016x}, glibc {:04x}:{:016x}: {d} ulp",
                ours.sign_exp, ours.significand, glibc.sign_exp, glibc.significand
            ));
        }
        Ok(())
    }

    /// mpmath's `log|gamma|` at 80 digits, correctly rounded, near each
    /// zero and pole below -2 -- at 1 to 10^4 ulps from the zero, 10^-15 to
    /// 1/4 from it, 1 ulp to 3/10 from the pole, on both sides -- and at 1,500
    /// random points in (-35, -2): `posix/tools/oracle/lgammal_zeros.py
    /// oracle`. Before `lgammal_near_zero`, the first rows were wrong in
    /// their leading digits; now all are within 3 ulps, most exact.
    #[test]
    fn lgammal_keeps_its_digits_near_the_zeros_below_minus_two() {
        // lgammal writes signgam, which math.rs's tests read.
        let _g = crate::math::signgam_test_lock();
        extended();
        let oracle = include_str!("lgammal_zero_oracle.txt");
        let line =
            |v: L| (i128::from(v.biased_exponent()) << 63) | i128::from(v.significand & !(1 << 63));
        let mut n = 0;
        let mut bad = Vec::new();
        for l in oracle.lines().filter(|l| !l.is_empty()) {
            let (xs, ys) = l.split_once(' ').expect("x y");
            let (x, want) = (parse_l(xs), parse_l(ys));
            let got = lgammal(x);
            let d = if got.is_sign_negative() == want.is_sign_negative() {
                (line(got) - line(want)).unsigned_abs()
            } else {
                u128::MAX
            };
            if d > 3 {
                bad.push(format!("{l}: {d} ulp"));
            }
            n += 1;
        }
        assert!(
            bad.is_empty(),
            "{} of {n}:
{}",
            bad.len(),
            bad.join(
                "
"
            )
        );
        assert_eq!(n, 3776);
    }

    fn check(line: &str) -> Result<(), String> {
        let (lhs, rhs) = line.split_once(" = ").ok_or("no =")?;
        let mut w = lhs.split(' ');
        let name = w.next().ok_or("no name")?;
        let ins: Vec<&str> = w.collect();
        let outs: Vec<&str> = rhs.split(' ').collect();
        let errno_glibc: i32 = outs[outs.len() - 1].parse().map_err(|_| "errno")?;
        let tol = ulps(name);
        errno::set_errno(0);
        let x = || parse_l(ins[0]);
        let y = || parse_l(ins[1]);
        let r = parse_l;
        let res = match name {
            "fabsl" => same(fabsl(x()), r(outs[0]), tol),
            "sqrtl" => same(sqrtl(x()), r(outs[0]), tol),
            "rintl" => same(rintl(x()), r(outs[0]), tol),
            "nearbyintl" => same(nearbyintl(x()), r(outs[0]), tol),
            "floorl" => same(floorl(x()), r(outs[0]), tol),
            "ceill" => same(ceill(x()), r(outs[0]), tol),
            "truncl" => same(truncl(x()), r(outs[0]), tol),
            "roundl" => same(roundl(x()), r(outs[0]), tol),
            "roundevenl" => same(roundevenl(x()), r(outs[0]), tol),
            "logbl" => same(logbl(x()), r(outs[0]), tol),
            "logl" => same(logl(x()), r(outs[0]), tol),
            "log2l" => same(log2l(x()), r(outs[0]), tol),
            "log10l" => same(log10l(x()), r(outs[0]), tol),
            "log1pl" => same(log1pl(x()), r(outs[0]), tol),
            "atanl" => same(atanl(x()), r(outs[0]), tol),
            "asinl" => same(asinl(x()), r(outs[0]), tol),
            "acosl" => same(acosl(x()), r(outs[0]), tol),
            "exp2l" => same(exp2l(x()), r(outs[0]), tol),
            "expl" => same(expl(x()), r(outs[0]), tol),
            "expm1l" => same(expm1l(x()), r(outs[0]), tol),
            "sinl" => same(sinl(x()), r(outs[0]), tol),
            "sinhl" => same(sinhl(x()), r(outs[0]), tol),
            "coshl" => same(coshl(x()), r(outs[0]), tol),
            "tanhl" => same(tanhl(x()), r(outs[0]), tol),
            "asinhl" => same(asinhl(x()), r(outs[0]), tol),
            "acoshl" => same(acoshl(x()), r(outs[0]), tol),
            "atanhl" => same(atanhl(x()), r(outs[0]), tol),
            "cbrtl" => same(cbrtl(x()), r(outs[0]), tol),
            "exp10l" => same(exp10l(x()), r(outs[0]), tol),
            "erfl" => same(erfl(x()), r(outs[0]), tol),
            "erfcl" => same(erfcl(x()), r(outs[0]), tol),
            // Relative everywhere, the zeros below -2 included, since
            // lgammal_near_zero (known-issues.md,
            // D-POSIX-LGAMMA-LOSES-DIGITS-NEAR-NEGATIVE-ROOTS).
            "lgammal" => same(lgammal(x()), r(outs[0]), tol),
            "tgammal" => same(tgammal(x()), r(outs[0]), tol),
            "fmal" => same(fmal(x(), y(), parse_l(ins[2])), r(outs[0]), 0),
            "hypotl" => same(hypotl(x(), y()), r(outs[0]), tol),
            "powl" => same(powl(x(), y()), r(outs[0]), tol),
            "cosl" => same(cosl(x()), r(outs[0]), tol),
            "tanl" => same(tanl(x()), r(outs[0]), tol),
            "sincosl" => {
                let (sn, cs) = sincosl(x());
                same(sn, r(outs[0]), tol).and_then(|()| same(cs, r(outs[1]), tol))
            }
            "lrintl" | "llrintl" | "lroundl" | "llroundl" | "ilogbl" => {
                let want: i64 = outs[0].parse().map_err(|_| "int")?;
                let got = match name {
                    "lrintl" => lrintl(x()),
                    "llrintl" => llrintl(x()),
                    "lroundl" => lroundl(x()),
                    "llroundl" => llroundl(x()),
                    _ => i64::from(ilogbl(x())),
                };
                if got == want {
                    Ok(())
                } else {
                    Err(format!("{got}, glibc {want}"))
                }
            }
            "fmodl" => same(fmodl(x(), y()), r(outs[0]), tol),
            "remainderl" => same(remainderl(x(), y()), r(outs[0]), tol),
            "atan2l" => same(atan2l(x(), y()), r(outs[0]), tol),
            "fdiml" => same(fdiml(x(), y()), r(outs[0]), tol),
            "fmaxl" => same(fmaxl(x(), y()), r(outs[0]), tol),
            "fminl" => same(fminl(x(), y()), r(outs[0]), tol),
            "copysignl" => same(copysignl(x(), y()), r(outs[0]), tol),
            "nextafterl" => same(nextafterl(x(), y()), r(outs[0]), tol),
            "nexttowardl" => same(nexttowardl(x(), y()), r(outs[0]), tol),
            "remquol" => {
                let (v, q) = remquol(x(), y());
                let want_q: i32 = outs[1].parse().map_err(|_| "quo")?;
                same(v, r(outs[0]), tol).and_then(|()| {
                    // glibc answers the quotient's low bits only for a finite
                    // result; C leaves it unspecified otherwise.
                    if !v.is_finite()
                        || (q & 7) == (want_q & 7) && (q < 0) == (want_q < 0)
                        || want_q == 0 && q == 0
                    {
                        Ok(())
                    } else {
                        Err(format!("quo {q}, glibc {want_q}"))
                    }
                })
            }
            "frexpl" => {
                let (v, e) = frexpl(x());
                let want_e: i32 = outs[1].parse().map_err(|_| "exp")?;
                same(v, r(outs[0]), 0).and_then(|()| {
                    if !v.is_finite() || e == want_e {
                        Ok(())
                    } else {
                        Err(format!("exp {e}, glibc {want_e}"))
                    }
                })
            }
            "modfl" => {
                let (f, i) = modfl(x());
                same(f, r(outs[0]), 0).and_then(|()| same(i, r(outs[1]), 0))
            }
            "ldexpl" | "scalbnl" | "scalblnl" => {
                let n: i64 = ins[1].parse().map_err(|_| "n")?;
                let v = match name {
                    "scalblnl" => scalblnl(x(), n),
                    _ => scalbnl(x(), i32::try_from(n).map_err(|_| "n range")?),
                };
                same(v, r(outs[0]), 0)
            }
            "nexttoward" => {
                let xd = f64::from_bits(u64::from_str_radix(ins[0], 16).map_err(|_| "x")?);
                let want = u64::from_str_radix(outs[0], 16).map_err(|_| "r")?;
                let got = nexttoward(xd, y());
                if got.to_bits() == want || got.is_nan() && f64::from_bits(want).is_nan() {
                    Ok(())
                } else {
                    Err(format!("{got:e}, glibc {:e}", f64::from_bits(want)))
                }
            }
            "nexttowardf" => {
                let xf = f32::from_bits(u32::from_str_radix(ins[0], 16).map_err(|_| "x")?);
                let want = u32::from_str_radix(outs[0], 16).map_err(|_| "r")?;
                let got = nexttowardf(xf, y());
                if got.to_bits() == want || got.is_nan() && f32::from_bits(want).is_nan() {
                    Ok(())
                } else {
                    Err(format!("{got:e}, glibc {:e}", f32::from_bits(want)))
                }
            }
            other => return Err(format!("no replay for {other}")),
        };
        res?;
        let ours = errno::get_errno();
        if ours != errno_glibc {
            return Err(format!("errno {ours}, glibc {errno_glibc}"));
        }
        Ok(())
    }

    /// Every function answers every call as glibc 2.39 does:
    /// bit for bit where IEEE fixes the answer, within [`ulps`] otherwise,
    /// with glibc's `errno`.
    #[test]
    fn every_call_answers_as_glibc_does() {
        // lgammal writes signgam, which math.rs's tests read.
        let _g = crate::math::signgam_test_lock();
        extended();
        let mut bad = Vec::new();
        let mut n = 0usize;
        for line in ORACLE.lines().filter(|l| !l.is_empty()) {
            n += 1;
            if let Err(e) = check(line) {
                bad.push(format!("{line}\n    {e}"));
            }
        }
        assert!(n > 10_000, "only {n} calls replayed");
        assert!(
            bad.is_empty(),
            "{} of {n} calls differ:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// Rounds in `mode` -- both units, with the x87 at 64 bits -- until
    /// dropped, then to nearest again.
    struct Rounding;

    impl Rounding {
        fn set(mode: i32) -> Self {
            extended();
            assert_eq!(crate::fenv::fesetround(mode), 0);
            Self
        }
    }

    impl Drop for Rounding {
        fn drop(&mut self) {
            crate::fenv::fesetround(crate::fenv::FE_TONEAREST);
        }
    }

    /// The functions computed to nearest in every direction.
    const TO_NEAREST: [&str; 8] = [
        "sinl", "cosl", "tanl", "sincosl", "powl", "exp10l", "lgammal", "tgammal",
    ];

    /// A one-argument `long double` function.
    type Unary = fn(L) -> L;

    /// The three directed modes, by name.
    const DIRECTED: [(&str, i32); 3] = [
        ("down", crate::fenv::FE_DOWNWARD),
        ("up", crate::fenv::FE_UPWARD),
        ("zero", crate::fenv::FE_TOWARDZERO),
    ];

    /// The functions computed to nearest answer in every direction exactly
    /// what they answer to nearest: before, `tanl` of pi/2 less 2^-51
    /// rounding toward zero was 7.6 million ulps out, and `tgammal` near 1
    /// rounding downward 6.
    #[test]
    fn computed_to_nearest_in_every_direction() {
        // lgammal writes signgam, which math.rs's tests read.
        let _g = crate::math::signgam_test_lock();
        let xs = [
            L::from_bits(0x3FFF, 0xC90F_DAA2_2168_C000),
            ld(core::f64::consts::PI),
            ld(-7.25),
            ld(0.3),
            ld(1e10),
            L::from_bits(0x3FFE, u64::MAX),
            ld(-2.5),
            ld(30.5),
        ];
        let unary: [(&str, Unary); 6] = [
            ("sinl", sinl),
            ("cosl", cosl),
            ("tanl", tanl),
            ("exp10l", exp10l),
            ("lgammal", lgammal),
            ("tgammal", tgammal),
        ];
        let bits = |v: L| (v.sign_exp, v.significand);
        for x in xs {
            extended();
            let near: Vec<_> = unary.iter().map(|(_, g)| bits(g(x))).collect();
            let near_sc = sincosl(x);
            let near_pow = powl(ld(1.5), x);
            for (mode_name, mode) in DIRECTED {
                let _r = Rounding::set(mode);
                for ((name, g), want) in unary.iter().zip(&near) {
                    // An overflow or underflow answers as the direction
                    // rounds instead (the next test).
                    let edge = L::from_bits(want.0, want.1);
                    if edge.is_infinite() || edge.is_zero() {
                        continue;
                    }
                    assert_eq!(bits(g(x)), *want, "{name}({x:?}) rounding {mode_name}");
                }
                let (s, c) = sincosl(x);
                assert_eq!((bits(s), bits(c)), (bits(near_sc.0), bits(near_sc.1)));
                if near_pow.is_finite() && !near_pow.is_zero() {
                    assert_eq!(
                        bits(powl(ld(1.5), x)),
                        bits(near_pow),
                        "powl(1.5, {x:?}) {mode_name}"
                    );
                }
            }
        }
    }

    /// An overflow or underflow answers what IEEE 754 prescribes for the
    /// direction, with `ERANGE`: `expl(11357.25)` rounding downward was
    /// 2,502 ulps below `LDBL_MAX`.
    #[test]
    fn overflow_and_underflow_answer_as_the_direction_rounds() {
        let max = L::from_bits(0x7FFE, u64::MAX);
        let tiny = L::from_bits(0, 1);
        let inf = L::INFINITY;
        //                              down  up    zero
        let cases: [(&str, Unary, L, [L; 3]); 5] = [
            ("expl", expl, ld(11357.25), [max, inf, max]),
            ("expl", expl, ld(-12000.0), [ZERO, tiny, ZERO]),
            ("exp2l", exp2l, ld(-16500.0), [ZERO, tiny, ZERO]),
            ("sinhl", sinhl, ld(-12000.0), [-inf, -max, -max]),
            ("exp10l", exp10l, ld(5000.0), [max, inf, max]),
        ];
        for (name, g, x, want) in cases {
            for ((mode_name, mode), w) in DIRECTED.into_iter().zip(want) {
                let _r = Rounding::set(mode);
                errno::set_errno(0);
                let got = g(x);
                assert_eq!(
                    (got.sign_exp, got.significand),
                    (w.sign_exp, w.significand),
                    "{name}({x:?}) rounding {mode_name}: {got:?}"
                );
                assert_eq!(
                    errno::get_errno(),
                    errno::ERANGE,
                    "{name}({x:?}) {mode_name}"
                );
            }
        }
    }

    /// The exact answers made before the algorithm, in every direction:
    /// `acoshl(1)` +0, `atanhl(+-1)` an infinity, `lgammal(-1)` +inf (all
    /// three were wrong rounding downward), `lgammal(1)` and `lgammal(2)` +0
    /// (Annex F) -- and `log1pl(LDBL_MAX)` finite, which rounding upward was
    /// an infinity.
    #[test]
    fn exact_answers_in_every_direction() {
        // lgammal writes signgam, which math.rs's tests read.
        let _g = crate::math::signgam_test_lock();
        let max = L::from_bits(0x7FFE, u64::MAX);
        extended();
        let log1p_max = log1pl(max);
        for (mode_name, mode) in DIRECTED {
            let _r = Rounding::set(mode);
            assert_eq!(acoshl(ONE).sign_exp, 0, "acoshl(1) {mode_name}");
            assert!(acoshl(ONE).is_zero());
            for x in [ONE, -ONE] {
                errno::set_errno(0);
                let r = atanhl(x);
                assert!(
                    r.is_infinite() && r.is_sign_negative() == x.is_sign_negative(),
                    "atanhl({x:?}) {mode_name}: {r:?}"
                );
                assert_eq!(errno::get_errno(), errno::ERANGE);
            }
            errno::set_errno(0);
            let r = lgammal(-ONE);
            assert!(
                r.is_infinite() && !r.is_sign_negative(),
                "lgammal(-1) {mode_name}: {r:?}"
            );
            assert_eq!(errno::get_errno(), errno::ERANGE);
            for x in [ONE, ld(2.0)] {
                let r = lgammal(x);
                assert_eq!(
                    (r.sign_exp, r.significand),
                    (0, 0),
                    "lgammal({x:?}) {mode_name}"
                );
            }
            let r = log1pl(max);
            assert!(r.is_finite(), "log1pl(LDBL_MAX) {mode_name}: {r:?}");
            let d = (i128::from(r.significand) - i128::from(log1p_max.significand)).abs();
            assert!(
                r.sign_exp == log1p_max.sign_exp && d <= 1,
                "log1pl(LDBL_MAX) {mode_name}: {r:?}"
            );
        }
    }

    /// What a call answers rounding in `mode`, given glibc's answers to
    /// nearest and in that mode: glibc's, but with `errno` the one to nearest
    /// where that is set (the rule `math.rs`'s module documentation gives,
    /// design-decisions.md section 1139), and an underflow -- zero and
    /// `ERANGE` to nearest -- answered as IEEE 754 prescribes for the
    /// direction. A function computed to nearest ([`TO_NEAREST`]) answers
    /// what glibc's does to nearest wherever that is finite and nonzero:
    /// glibc's own `lgammal` rounding downward is up to 7 ulps from its
    /// answer to nearest. And `lgammal(1)` and `lgammal(2)` are +0 in every
    /// direction (Annex F, F.10.5.3), where glibc's `lgammal(2)` rounding
    /// downward is -0.
    fn directed_answer(mode: &str, name: &str, x: &str, near: &str, glibc: &str) -> String {
        let near_outs: Vec<&str> = near.split(' ').collect();
        let inside = near_outs[0].contains(':') && {
            let v = parse_l(near_outs[0]);
            v.is_finite() && !v.is_zero()
        };
        let glibc = if TO_NEAREST.contains(&name) && inside {
            near
        } else {
            glibc
        };
        let mut outs: Vec<String> = glibc.split(' ').map(str::to_owned).collect();
        let near_errno = *near_outs.last().expect("an errno");
        if near_errno != "0" {
            *outs.last_mut().expect("an errno") = near_errno.to_owned();
        }
        let rounds = !matches!(
            name,
            "nextafterl" | "nexttowardl" | "nexttoward" | "nexttowardf"
        );
        if near_outs[0].contains(':') && rounds && near_errno == "34" {
            let v = parse_l(near_outs[0]);
            if v.is_zero() {
                let negative = v.is_sign_negative();
                let away = matches!((mode, negative), ("up", false) | ("down", true));
                let sign = if negative { 0x8000 } else { 0 };
                outs[0] = format!("{sign:04x}:{:016x}", u64::from(away));
            }
        }
        if name == "lgammal" && (parse_l(x) == ONE || parse_l(x) == ld(2.0)) {
            outs[0] = "0000:0000000000000000".to_owned();
        }
        outs.join(" ")
    }

    /// Every call of [`ORACLE`] again in each directed rounding mode, against
    /// [`directed_answer`]: bit for bit where IEEE fixes the answer, within
    /// the bound to nearest and one ulp more otherwise.
    #[test]
    fn every_call_answers_in_every_rounding_direction() {
        // lgammal writes signgam, which math.rs's tests read.
        let _g = crate::math::signgam_test_lock();
        let mut directed = std::collections::HashMap::new();
        for line in MODES_ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (mode, call) = line.split_once(' ').expect("a mode");
            let (lhs, rhs) = call.split_once(" = ").expect("an =");
            directed.insert((mode, lhs), rhs);
        }
        let mut bad = Vec::new();
        let mut n = 0usize;
        let mut listed = std::collections::HashSet::new();
        for (mode_name, mode) in [
            ("down", crate::fenv::FE_DOWNWARD),
            ("up", crate::fenv::FE_UPWARD),
            ("zero", crate::fenv::FE_TOWARDZERO),
        ] {
            for line in ORACLE.lines().filter(|l| !l.is_empty()) {
                let (lhs, near) = line.split_once(" = ").expect("an =");
                let glibc = directed.get(&(mode_name, lhs)).copied();
                if glibc.is_some() {
                    listed.insert((mode_name, lhs));
                }
                let mut words = lhs.split(' ');
                let name = words.next().expect("a name");
                let x = words.next().expect("an argument");
                let want = directed_answer(mode_name, name, x, near, glibc.unwrap_or(near));
                n += 1;
                DIRECTED_SLACK.with(|s| s.set(1));
                let result = {
                    let _r = Rounding::set(mode);
                    check(&format!("{lhs} = {want}"))
                };
                DIRECTED_SLACK.with(|s| s.set(0));
                if let Err(e) = result {
                    bad.push(format!(
                        "{mode_name} {lhs} = {want}\n    to nearest {near}; glibc {mode_name} {}\n    {e}",
                        glibc.unwrap_or("as to nearest")
                    ));
                }
            }
        }
        assert_eq!(
            listed.len(),
            directed.len(),
            "every directed line is a call of ORACLE's"
        );
        assert!(n > 90_000, "only {n} calls replayed");
        assert!(
            bad.is_empty(),
            "{} of {n} calls differ in a directed mode:\n{}",
            bad.len(),
            bad.iter()
                .take(5000)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// glibc 2.39's answers over every class of 80-bit encoding -- zeros,
    /// subnormals, pseudo-denormals, normals, unnormals, a pseudo-zero,
    /// infinities, a pseudo-infinity, a pseudo-NaN, quiet and signalling
    /// NaNs -- and for `nanl`'s tags (from `posix/oracle/ldclass.c`):
    /// `class x = __fpclassifyl __signbitl finitel isinfl isnanl`,
    /// `significandl x = result errno`, `nanl "tag" = result`.
    const CLASS_ORACLE: &str = r#"
class 0000:0000000000000000 = 2 0 1 0 0
class 8000:0000000000000000 = 2 512 1 0 0
class 0000:0000000000000001 = 3 0 1 0 0
class 8000:7FFFFFFFFFFFFFFF = 3 512 1 0 0
class 0000:8000000000000000 = 4 0 1 0 0
class 8000:C000000000000001 = 4 512 1 0 0
class 0001:8000000000000000 = 4 0 1 0 0
class 3FFF:8000000000000000 = 4 0 1 0 0
class BFFF:C90FDAA22168C235 = 4 512 1 0 0
class 7FFE:FFFFFFFFFFFFFFFF = 4 0 1 0 0
class 3FFF:4000000000000000 = 0 0 1 0 1
class 3FFF:0000000000000000 = 0 0 1 0 1
class 7FFF:8000000000000000 = 1 0 0 1 0
class FFFF:8000000000000000 = 1 512 0 -1 0
class 7FFF:0000000000000000 = 0 0 0 0 1
class FFFF:4000000000000000 = 0 512 0 0 65535
class 7FFF:C000000000000000 = 0 0 0 0 65535
class FFFF:C000000000000001 = 0 512 0 0 65535
class 7FFF:8000000000000001 = 0 0 0 0 65535
class 7FFF:BFFFFFFFFFFFFFFF = 0 0 0 0 65535
significandl 0000:0000000000000000 = 0000:0000000000000000 0
significandl 8000:0000000000000000 = 8000:0000000000000000 0
significandl 0000:0000000000000001 = 3FFF:8000000000000000 0
significandl 8000:7FFFFFFFFFFFFFFF = BFFF:FFFFFFFFFFFFFFFE 0
significandl 0000:8000000000000000 = 3FFF:8000000000000000 0
significandl 8000:C000000000000001 = BFFF:C000000000000001 0
significandl 0001:8000000000000000 = 3FFF:8000000000000000 0
significandl 3FFF:8000000000000000 = 3FFF:8000000000000000 0
significandl BFFF:C90FDAA22168C235 = BFFF:C90FDAA22168C235 0
significandl 7FFE:FFFFFFFFFFFFFFFF = 3FFF:FFFFFFFFFFFFFFFF 0
significandl 3FFF:4000000000000000 = FFFF:C000000000000000 0
significandl 3FFF:0000000000000000 = FFFF:C000000000000000 0
significandl 7FFF:8000000000000000 = 7FFF:8000000000000000 0
significandl FFFF:8000000000000000 = FFFF:8000000000000000 0
significandl 7FFF:0000000000000000 = FFFF:C000000000000000 0
significandl FFFF:4000000000000000 = FFFF:C000000000000000 0
significandl 7FFF:C000000000000000 = 7FFF:C000000000000000 0
significandl FFFF:C000000000000001 = FFFF:C000000000000001 0
significandl 7FFF:8000000000000001 = 7FFF:C000000000000001 0
significandl 7FFF:BFFFFFFFFFFFFFFF = 7FFF:FFFFFFFFFFFFFFFF 0
nanl "" = 7FFF:C000000000000000
nanl "0" = 7FFF:C000000000000000
nanl "1" = 7FFF:C000000000000001
nanl "123" = 7FFF:C00000000000007B
nanl "0x1" = 7FFF:C000000000000001
nanl "0X7f" = 7FFF:C00000000000007F
nanl "017" = 7FFF:C00000000000000F
nanl "0x3FFFFFFFFFFFFFFF" = 7FFF:FFFFFFFFFFFFFFFF
nanl "0x4000000000000000" = 7FFF:C000000000000000
nanl "0xFFFFFFFFFFFFFFFF" = 7FFF:FFFFFFFFFFFFFFFF
nanl "18446744073709551615" = 7FFF:FFFFFFFFFFFFFFFF
nanl "18446744073709551616" = 7FFF:FFFFFFFFFFFFFFFF
nanl "abc" = 7FFF:C000000000000000
nanl "_" = 7FFF:C000000000000000
nanl "12a" = 7FFF:C000000000000000
nanl "0x" = 7FFF:C000000000000000
nanl "-1" = 7FFF:C000000000000000
nanl " 1" = 7FFF:C000000000000000
nanl "1 " = 7FFF:C000000000000000
nanl "0x1p3" = 7FFF:C000000000000000
nanl "9999999999999999999999" = 7FFF:FFFFFFFFFFFFFFFF
"#;

    fn bits(v: L) -> (u16, u64) {
        (v.sign_exp, v.significand)
    }

    #[test]
    fn classification_significandl_and_nanl_answer_as_glibc_does() {
        extended();
        let mut n = 0;
        for line in CLASS_ORACLE.lines().filter(|l| !l.is_empty()) {
            let (lhs, rhs) = line.split_once(" = ").expect("=");
            let (name, arg) = lhs.split_once(' ').expect("name");
            match name {
                "class" => {
                    let x = parse_l(arg);
                    let want: Vec<i32> = rhs.split(' ').map(|v| v.parse().expect("int")).collect();
                    let got = [
                        fpclassifyl(x),
                        signbitl(x),
                        finitel(x),
                        isinfl(x),
                        isnanl(x),
                    ];
                    assert_eq!(got[..], want[..], "{line}");
                }
                "significandl" => {
                    let (want, e) = rhs.split_once(' ').expect("errno");
                    errno::set_errno(0);
                    let got = significandl(parse_l(arg));
                    assert_eq!(bits(got), bits(parse_l(want)), "{line}");
                    assert_eq!(
                        errno::get_errno(),
                        e.parse::<i32>().expect("errno"),
                        "{line}"
                    );
                }
                "nanl" => {
                    let tag = format!("{}\0", arg.trim_matches('"'));
                    // SAFETY: a NUL-terminated string.
                    let got = unsafe { nanl(tag.as_ptr()) };
                    assert_eq!(bits(got), bits(parse_l(rhs)), "{line}");
                }
                other => panic!("no replay for {other}"),
            }
            n += 1;
        }
        assert_eq!(n, 61);
    }

    #[test]
    fn nanl_of_null_is_the_default_nan() {
        // SAFETY: NULL is allowed.
        assert_eq!(
            bits(unsafe { nanl(core::ptr::null()) }),
            (0x7FFF, 0xC000_0000_0000_0000)
        );
    }

    /// The quotient is the *rounded* one, though `remquol` reads it off the
    /// truncating `fprem`: one further exactly when the two remainders
    /// differ, ties to even. The oracle replay covers this on hardware; this
    /// spells out each kind of case. QEMU, whose `fprem1` reports no
    /// quotient at all, is covered from ring 3 by `ctest-longdouble` 72.
    #[test]
    fn remquol_rounds_the_truncated_quotient() {
        extended();
        for (x, y, r, q) in [
            (10.0, 3.0, 1.0, 3),   // 3.33: truncated is already nearest
            (11.0, 3.0, -1.0, 4),  // 3.67: one further, and past zero
            (7.5, 3.0, 1.5, 2),    // 2.5, a tie, and 2 is even
            (4.5, 3.0, -1.5, 2),   // 1.5, a tie, and 1 is odd
            (-11.0, 3.0, 1.0, -4), // the sign is the signs' product
            (11.0, -3.0, -1.0, -4),
            (-11.0, -3.0, 1.0, 4),
            (6.0, 3.0, 0.0, 2),
            (7.0, 1.0, 0.0, 7),
            (9.0, 1.0, 0.0, 1),   // the low three bits only: 0b1001
            (15.5, 1.0, -0.5, 0), // 16, to even: 0b10000
        ] {
            let (got_r, got_q) = remquol(ld(x), ld(y));
            assert_eq!((bits(got_r), got_q), (bits(ld(r)), q), "remquol({x}, {y})");
        }
    }

    /// `fxtract`, which `significandl` is, on the cases the oracle's
    /// table does not spell out: the exponent half.
    #[test]
    fn fxtract_splits_as_the_unit_does() {
        extended();
        let x = L::from_bits(0x4003, 0xA000_0000_0000_0000); // 20
        let (s, e) = x.fxtract();
        assert_eq!(bits(s), (0x3FFF, 0xA000_0000_0000_0000)); // 1.25
        assert_eq!(bits(e), bits(L::from_i64(4)));
        let (s, e) = L::POS_ZERO.negate().fxtract();
        assert_eq!(bits(s), (0x8000, 0));
        assert_eq!(bits(e), bits(L::INFINITY.negate()));
        let (s, e) = L::INFINITY.fxtract();
        assert_eq!(bits(s), bits(L::INFINITY));
        assert_eq!(bits(e), bits(L::INFINITY));
        // The least subnormal: 1 * 2^-16445.
        let (s, e) = L::from_bits(0, 1).fxtract();
        assert_eq!(bits(s), bits(ONE));
        assert_eq!(bits(e), bits(L::from_i64(-16445)));
    }
}

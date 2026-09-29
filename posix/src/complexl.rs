//! `<complex.h>` for `long double complex`: `cabsl` and the other 21, and
//! `__mulxc3`/`__divxc3`, the helpers C compilers call to multiply and divide
//! two of them.
//!
//! The `double` and `float` functions are `complex.rs`'s; these are their
//! 80-bit twins, computing on the x87 unit through [`crate::ld80`] and the
//! `long double` real functions of [`crate::mathl`].
//!
//! # Where the code comes from
//!
//! FreeBSD's msun, release 14.1.0, as `complex.rs`'s does: `catrigl.c`
//! (Stephen Montgomery-Smith, Mahdi Mokhtari) for the six inverse functions,
//! `s_clogl.c` (Bruce Evans) for `clogl`, `s_csqrtl.c` (David Schultz) and the
//! ld80 `s_cexpl.c` (Steven Kargl) -- ported line for line, BSD-2-Clause, the
//! notices at each section. FreeBSD has no `long double` `ccoshl`, `csinhl`,
//! `ctanhl` or their circular twins: those are `complex.rs`'s `double`
//! algorithms (Schultz's and Kargl's `s_ccosh.c`, `s_csinh.c`, `s_ctanh.c`)
//! carried to the 80-bit format, with thresholds worked out for it where the
//! `double` code tests bit patterns. `cpowl` is glibc's definition, as `cpow`
//! is: `cexpl(y * clogl(x))` with the product formed as `__mulxc3` forms it.
//!
//! # Every rounding direction
//!
//! As `complex.rs`'s: the real functions they are built from are
//! `mathl.rs`'s, right in every direction, and where an `e^x` or `cosh x`
//! that has overflowed is multiplied by a sine or cosine, the overflow is
//! answered as the direction rounds the signed result (`overflowed`) --
//! rounding downward, `ccoshl(LDBL_MAX - 2i)` was two finite numbers where
//! the direction owes -inf. Replayed against glibc in all three directed
//! modes (`complexl_modes_oracle.txt`).
//!
//! # ABI
//!
//! A `long double complex` is class COMPLEX_X87: an argument is passed in
//! memory, 32 bytes -- the real part's 16-byte slot, then the imaginary
//! part's -- and a result comes back on the x87 stack, the real part in
//! `%st(0)` and the imaginary part in `%st(1)`. Rust can express neither, so
//! every C entry point is an assembly thunk ([`crate::ld_c`]) around a Rust
//! function taking pointers to its arguments and to a result slot.
//! `__mulxc3` and `__divxc3` take their four parts as four `long double`
//! arguments, in memory.

// `LongDouble`'s operators are x87 floating-point operations, as in
// mathl.rs: they round, overflow to infinity and propagate NaN, and cannot
// panic or wrap, which is what this lint looks for.
#![allow(clippy::arithmetic_side_effects)]
// `y - y` and `(b - b) / (b - b)` are Annex G's idiom, as in complex.rs: a
// NaN made *from* an operand, raising invalid for an infinity as the
// standard asks, where a constant NaN would raise nothing.
#![allow(clippy::eq_op)]

use crate::x87::LongDouble as L;

/// A `long double complex` as it lies in memory: two 16-byte slots, the real
/// part first.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LdComplex {
    pub re: L,
    pub im: L,
}

/// `re + i im`.
const fn cl(re: L, im: L) -> LdComplex {
    LdComplex { re, im }
}

/// A `long double` from an `f64`, exactly.
fn ld(x: f64) -> L {
    L::from_f64(x)
}

/// `2^e`, exactly, for `e` in the normal range.
const fn p2l(e: i32) -> L {
    // A biased exponent in 1..=0x7FFE: the callers pass constants.
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let field = (16383 + e) as u16;
    L::from_bits(field, 1 << 63)
}

/// `errno` as it was, put back when dropped. glibc's complex functions work
/// through its internal real functions, which set no `errno`, so none of
/// its `long double` ones sets it but `cabsl` and `cargl` (through `hypotl`
/// and `atan2l`); `mathl`'s real functions are the public ones, which do,
/// and each function here that calls them keeps `errno` as it found it.
struct KeepErrno(i32);

impl KeepErrno {
    fn new() -> Self {
        Self(crate::errno::get_errno())
    }
}

impl Drop for KeepErrno {
    fn drop(&mut self) {
        crate::errno::set_errno(self.0);
    }
}

const ZERO: L = L::POS_ZERO;
const ONE: L = L::ONE;
const INF: L = L::INFINITY;

// ===========================================================================
// The parts, the conjugate, the projection, the modulus and the argument
// ===========================================================================

/// The real part.
#[must_use]
pub fn creall(z: LdComplex) -> L {
    z.re
}

/// The imaginary part.
#[must_use]
pub fn cimagl(z: LdComplex) -> L {
    z.im
}

/// The complex conjugate: the imaginary part negated, NaN or not.
#[must_use]
pub fn conjl(z: LdComplex) -> LdComplex {
    cl(z.re, z.im.negate())
}

/// The projection onto the Riemann sphere: an infinite part in either place
/// makes `inf + i copysign(0, im)`; anything else is itself (FreeBSD
/// `s_cprojl.c`).
#[must_use]
pub fn cprojl(z: LdComplex) -> LdComplex {
    if z.re.is_infinite() || z.im.is_infinite() {
        cl(INF, ZERO.copysign(z.im))
    } else {
        z
    }
}

/// The modulus, `hypotl(re, im)` (FreeBSD `w_cabsl.c`) -- and so its
/// `errno`, `ERANGE` when it overflows, as glibc's.
#[must_use]
pub fn cabsl(z: LdComplex) -> L {
    crate::mathl::hypotl(z.re, z.im)
}

/// The argument, `atan2l(im, re)` (FreeBSD `s_cargl.c`).
#[must_use]
pub fn cargl(z: LdComplex) -> L {
    crate::mathl::atan2l(z.im, z.re)
}

// ===========================================================================
// __mulxc3, __divxc3 -- the compiler's multiplication and division
// ===========================================================================

/// An infinity as 1 with its sign, anything else as 0 with its sign: Annex
/// G's recovery step.
fn unit_if_inf(x: L) -> L {
    (if x.is_infinite() { ONE } else { ZERO }).copysign(x)
}

/// A NaN as 0 with its sign, anything else unchanged.
fn zero_if_nan(x: L) -> L {
    if x.is_nan() { ZERO.copysign(x) } else { x }
}

/// `(a + bi) * (c + di)` as C99 Annex G.5.1 multiplies: the four products,
/// and when both parts come out NaN, the infinities recovered -- an infinite
/// operand times a nonzero finite one is infinite, not NaN. What a C
/// compiler calls for `long double complex` `*`.
#[must_use]
pub fn mulxc3(a: L, b: L, c: L, d: L) -> LdComplex {
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let ac = a * c;
    let bd = b * d;
    let ad = a * d;
    let bc = b * c;
    let mut z = cl(ac - bd, ad + bc);
    if z.re.is_nan() && z.im.is_nan() {
        let mut recalc = false;
        if a.is_infinite() || b.is_infinite() {
            a = unit_if_inf(a);
            b = unit_if_inf(b);
            c = zero_if_nan(c);
            d = zero_if_nan(d);
            recalc = true;
        }
        if c.is_infinite() || d.is_infinite() {
            c = unit_if_inf(c);
            d = unit_if_inf(d);
            a = zero_if_nan(a);
            b = zero_if_nan(b);
            recalc = true;
        }
        if !recalc && (ac.is_infinite() || bd.is_infinite() || ad.is_infinite() || bc.is_infinite())
        {
            a = zero_if_nan(a);
            b = zero_if_nan(b);
            c = zero_if_nan(c);
            d = zero_if_nan(d);
            recalc = true;
        }
        if recalc {
            z.re = INF * (a * c - b * d);
            z.im = INF * (a * d + b * c);
        }
    }
    z
}

/// `(a + bi) / (c + di)` as C99 Annex G.5.1 divides: the divisor scaled by a
/// power of two near its magnitude against overflow and underflow, and the
/// infinities and zeros recovered when the quotient comes out NaN. What a C
/// compiler calls for `long double complex` `/`.
#[must_use]
pub fn divxc3(a: L, b: L, c: L, d: L) -> LdComplex {
    let _keep = KeepErrno::new();
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let mut ilogbw = 0_i32;
    let logbw = crate::mathl::logbl(crate::mathl::fmaxl(c.abs(), d.abs()));
    if logbw.is_finite() {
        // A finite `logbl` of a `long double` is within +-16445.
        ilogbw = i32::try_from(logbw.to_i64_rint()).unwrap_or(0);
        c = c.scalbn(ilogbw.saturating_neg());
        d = d.scalbn(ilogbw.saturating_neg());
    }
    let denom = c * c + d * d;
    let mut z = cl(
        ((a * c + b * d) / denom).scalbn(ilogbw.saturating_neg()),
        ((b * c - a * d) / denom).scalbn(ilogbw.saturating_neg()),
    );
    if z.re.is_nan() && z.im.is_nan() {
        if denom.is_zero() && (!a.is_nan() || !b.is_nan()) {
            z.re = INF.copysign(c) * a;
            z.im = INF.copysign(c) * b;
        } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
            a = unit_if_inf(a);
            b = unit_if_inf(b);
            z.re = INF * (a * c + b * d);
            z.im = INF * (b * c - a * d);
        } else if logbw.is_infinite() && logbw > ZERO && a.is_finite() && b.is_finite() {
            c = unit_if_inf(c);
            d = unit_if_inf(d);
            z.re = ZERO * (a * c + b * d);
            z.im = ZERO * (b * c - a * d);
        }
    }
    z
}

// ===========================================================================
// csqrtl -- FreeBSD s_csqrtl.c
//
// Copyright (c) 2007-2008 David Schultz <das@FreeBSD.ORG>
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// The square root, with its branch cut along the negative real axis: the
/// real part is never negative, and the imaginary part has the sign of `z`'s.
#[must_use]
pub fn csqrtl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    // Overflow must be avoided for components >= LDBL_MAX / (1 + sqrt(2)):
    // a lower threshold of about LDBL_MAX / 4.
    let thresh = p2l(16382);
    let (mut a, mut b) = (z.re, z.im);

    // Handle special cases.
    if a.is_zero() && b.is_zero() {
        return cl(ZERO, b);
    }
    if b.is_infinite() {
        return cl(INF, b);
    }
    if a.is_nan() {
        let t = (b - b) / (b - b); // raise invalid if b is not a NaN
        return cl(a + t, a + t); // NaN + NaN i
    }
    if a.is_infinite() {
        // csqrt(inf + NaN i)  = inf +  NaN i
        // csqrt(inf + y i)    = inf +  0 i
        // csqrt(-inf + NaN i) = NaN +- inf i
        // csqrt(-inf + y i)   = 0   +  inf i
        return if a.is_sign_negative() {
            cl((b - b).abs(), a.copysign(b))
        } else {
            cl(a, (b - b).copysign(b))
        };
    }
    if b.is_nan() {
        let t = (a - a) / (a - a); // raise invalid
        return cl(b + t, b + t); // NaN + NaN i
    }

    // Scale to avoid overflow.
    let mut scale = if a.abs() >= thresh || b.abs() >= thresh {
        // Don't scale a or b if this might give (spurious) underflow. Then
        // the unscaled value is an equivalent infinitesimal (or 0).
        if a.abs() >= p2l(-16380) {
            a = a * ld(0.25);
        }
        if b.abs() >= p2l(-16380) {
            b = b * ld(0.25);
        }
        ld(2.0)
    } else {
        ONE
    };

    // Scale to reduce inaccuracies when both components are denormal.
    if a.abs() < p2l(-16382) && b.abs() < p2l(-16382) {
        a = a * p2l(64);
        b = b * p2l(64);
        scale = p2l(-32);
    }

    // Algorithm 312, CACM vol 10, Oct 1967.
    let half = ld(0.5);
    let two = ld(2.0);
    if a >= ZERO {
        let t = ((a + crate::mathl::hypotl(a, b)) * half).sqrt();
        cl(scale * t, scale * b / (two * t))
    } else {
        let t = ((a.negate() + crate::mathl::hypotl(a, b)) * half).sqrt();
        cl(scale * b.abs() / (two * t), (scale * t).copysign(b))
    }
}

/// An overflow with `sign`'s sign, as the x87 unit's rounding direction
/// gives it -- an infinity, or `LDBL_MAX` where the direction rounds toward
/// zero ([`crate::math::Real::overflow`]). For the branches where FreeBSD
/// multiplies an `e^x` or `cosh x` that has already overflowed by a sine or
/// cosine: rounding downward the overflowed factor is `LDBL_MAX`, and
/// `LDBL_MAX * cos(1)` a finite number far below it.
fn overflowed(sign: L) -> L {
    <L as crate::math::Real>::overflow(sign)
}

/// `v` with the sign of `v * x`: negated for a negative `x`.
fn signed_by(v: L, x: L) -> L {
    if x.is_sign_negative() { v.negate() } else { v }
}

// ===========================================================================
// cexpl -- FreeBSD ld80/s_cexpl.c, and __ldexp_cexpl from ld80/k_expl.h
//
// Copyright (c) 2019 Steven G. Kargl
// Copyright (c) 2009-2013 Steven G. Kargl, Bruce D. Evans
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// `e^x` times `2^expt`, and `cos y`, `sin y` times it, for an `x` whose
/// `e^x` alone overflows while the product need not: FreeBSD's
/// `__ldexp_cexpl`. Its table-driven kernel gives `e^x` as `(hi + lo) 2^k`;
/// here `x` less a whole number `k` of `ln 2`s -- taken off in two parts,
/// the first with its low 32 bits zero so the product with `k` is exact --
/// leaves an `r` whose `expl` is near 1, and the result is scaled as
/// FreeBSD scales it: by `2^16382` first, then by the rest in two halves,
/// neither of which overflows on its own. `re_sign` and `im_sign` (each
/// +-1) multiply the parts before they are scaled: `ccoshl` and `csinhl`
/// want one negated, and negating after the scaling would round an overflow
/// the wrong way -- rounding downward, -`LDBL_MAX` where the direction owes
/// -inf.
fn ldexp_cexpl(x: L, y: L, expt: i32, re_sign: L, im_sign: L) -> LdComplex {
    const LN2_HI: L = L::from_bits(0x3FFE, 0xB172_17F7_0000_0000);
    const LN2_LO: L = L::from_bits(0x3FDE, 0xD1CF_79AB_C9E3_B398);
    const INV_LN2: L = L::from_bits(0x3FFF, 0xB8AA_3B29_5C17_F0BC);
    // `x` is below 22756 here, so `k` is below 2^15 and `k * LN2_HI` exact;
    // `x` and it are then within a factor of 2, so the difference is too.
    let k_int = (x * INV_LN2).to_i64_rint();
    let k = L::from_i64(k_int);
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let exp_x = crate::mathl::expl(r).scalbn(16382);
    let k_int = i32::try_from(k_int).unwrap_or(0);
    let expt = expt.saturating_add(k_int).saturating_sub(16382);
    let half = expt / 2;
    let scale1 = ONE.scalbn(half);
    let scale2 = ONE.scalbn(expt.saturating_sub(half));
    let (s, c) = crate::mathl::sincosl(y);
    let (c, s) = (c * re_sign, s * im_sign);
    cl(c * exp_x * scale1 * scale2, s * exp_x * scale1 * scale2)
}

/// The exponential, `e^re (cos im + i sin im)`; `ERANGE` when `e^re`
/// underflows to zero, where glibc's `expl` sets it.
#[must_use]
pub fn cexpl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);

    // cexp(x + I 0) = exp(x) + I 0
    if y.is_zero() {
        return cl(crate::mathl::expl(x), y);
    }
    // cexp(0 + I y) = cos(y) + I sin(y)
    if x.is_zero() {
        let (s, c) = crate::mathl::sincosl(y);
        return cl(c, s);
    }

    if !y.is_finite() {
        if x.is_finite() || x.is_nan() {
            // cexp(finite|NaN +- I Inf|NaN) = NaN + I NaN
            return cl(y - y, y - y);
        } else if x.is_sign_negative() {
            // cexp(-Inf +- I Inf|NaN) = 0 +- I 0: the zero takes y's sign, as
            // glibc's does (FreeBSD's is always +0).
            return cl(ZERO, ZERO.copysign(y));
        }
        // cexp(+Inf +- I Inf|NaN) = Inf + I NaN
        return cl(x, y - y);
    }

    // exp_ovfl  = 11356.5234062941439497, the largest x whose e^x is finite;
    // cexp_ovfl = 22755.3287906024445633, past which e^x * s overflows for
    // every s -- the bounds FreeBSD tests as bit patterns.
    let exp_ovfl = L::from_bits(0x400C, 0xB172_17F7_D1CF_79AC);
    let cexp_ovfl = L::from_bits(0x400D, 0xB1C6_A857_3DE9_768C);
    if x > exp_ovfl && x < cexp_ovfl {
        // x is between exp_ovfl and cexp_ovfl, so we must scale to avoid
        // overflow in exp(x).
        ldexp_cexpl(x, y, 0, ONE, ONE)
    } else if x >= cexp_ovfl && x.is_finite() {
        // e^x times cos(y) or sin(y), neither zero for a finite nonzero y,
        // overflows: answered as the rounding direction gives it
        // ([`overflowed`]).
        let (s, c) = crate::mathl::sincosl(y);
        cl(overflowed(c), overflowed(s))
    } else {
        // Cases covered here:
        //  -  x < exp_ovfl and exp(x) won't overflow (common case)
        //  -  x > cexp_ovfl, so exp(x) * s overflows for all s > 0
        //  -  x = +-Inf (generated by exp())
        //  -  x = NaN (spurious inexact exception from y)
        let exp_x = crate::mathl::expl(x);
        let (s, c) = crate::mathl::sincosl(y);
        cl(exp_x * c, exp_x * s)
    }
}

// ===========================================================================
// clogl -- FreeBSD s_clogl.c
//
// Copyright (c) 2013 Bruce D. Evans
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// `(a, b)` replaced by their sum, rounded, and the error of the rounding --
/// for `|a| >= |b|` (FreeBSD's `_2sumF`).
fn two_sum_f(a: &mut L, b: &mut L) {
    let w = *a + *b;
    *b = (*a - w) + *b;
    *a = w;
}

/// [`two_sum_f`] for any `a` and `b` (FreeBSD's `_2sum`).
fn two_sum(a: &mut L, b: &mut L) {
    let w = *a + *b;
    let bv = w - *a;
    *b = (*a - (w - bv)) + (*b - bv);
    *a = w;
}

/// The natural logarithm, `log|z| + i arg z`, with the branch cut along the
/// negative real axis -- `log|z|` accurate near `|z| = 1` by Evans' doubled
/// precision, where the textbook formula loses its digits.
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn clogl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    const MANT_DIG: i32 = 64;
    const MAX_EXP: i32 = 16384;
    const MIN_EXP: i32 = -16381;
    // ln 2 = LN2_HI + LN2L_LO, LN2_HI a double with its low bits zero.
    let ln2_hi = ld(6.931_471_805_582_987e-1);
    let ln2l_lo = ld(1.646_594_958_289_708_2e-12);
    // 2^32, MANT_DIG / 2 rounded up, for Dekker's split.
    let mult_redux = p2l(32);

    let (x, y) = (z.re, z.im);
    let v = crate::mathl::atan2l(y, x);

    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }
    let kx = i32::from(ax.biased_exponent()) - 16383;
    let ky = i32::from(ay.biased_exponent()) - 16383;

    // Handle NaNs and Infs using the general formula.
    if kx == MAX_EXP || ky == MAX_EXP {
        return cl(crate::mathl::logl(crate::mathl::hypotl(x, y)), v);
    }

    // Avoid spurious underflow, and reduce inaccuracies when ax is 1.
    if ax == ONE {
        if ky < (MIN_EXP - 1) / 2 {
            return cl((ay / ld(2.0)) * ay, v);
        }
        return cl(crate::mathl::log1pl(ay * ay) / ld(2.0), v);
    }

    // Avoid underflow when ax is not small. Also handle zero args.
    if kx - ky > MANT_DIG || ay.is_zero() {
        return cl(crate::mathl::logl(ax), v);
    }

    // Avoid overflow.
    if kx >= MAX_EXP - 1 {
        let n = L::from_i64(i64::from(MAX_EXP - 2));
        return cl(
            crate::mathl::logl(crate::mathl::hypotl(x * p2l(-16382), y * p2l(-16382)))
                + n * ln2l_lo
                + n * ln2_hi,
            v,
        );
    }
    if kx >= (MAX_EXP - 1) / 2 {
        return cl(crate::mathl::logl(crate::mathl::hypotl(x, y)), v);
    }

    // Reduce inaccuracies and avoid underflow when ax is denormal.
    if kx <= MIN_EXP - 2 {
        let n = L::from_i64(i64::from(MIN_EXP - 2));
        return cl(
            crate::mathl::logl(crate::mathl::hypotl(x * p2l(16383), y * p2l(16383)))
                + n * ln2l_lo
                + n * ln2_hi,
            v,
        );
    }

    // Avoid remaining underflows (when ax is small but not denormal).
    if ky < (MIN_EXP - 1) / 2 + MANT_DIG {
        return cl(crate::mathl::logl(crate::mathl::hypotl(x, y)), v);
    }

    // Calculate ax*ax and ay*ay exactly using Dekker's algorithm.
    let t = ax * (mult_redux + ONE);
    let axh = (ax - t) + t;
    let axl = ax - axh;
    let ax2h = ax * ax;
    let mut ax2l = axh * axh - ax2h + ld(2.0) * axh * axl + axl * axl;
    let t = ay * (mult_redux + ONE);
    let ayh = (ay - t) + t;
    let ayl = ay - ayh;
    let ay2h = ay * ay;
    let mut ay2l = ayh * ayh - ay2h + ld(2.0) * ayh * ayl + ayl * ayl;

    // When log|z| is far from 1, accuracy in the sum of the squares matters
    // little, since log reduces inaccuracies; when |z| is near 1, subtract 1
    // exactly in doubled precision and use log1p (Evans' comments in
    // s_clogl.c give the whole argument).
    let mut sh = ax2h;
    let mut sl = ay2h;
    two_sum_f(&mut sh, &mut sl);
    if sh < ld(0.5) || sh >= ld(3.0) {
        return cl(crate::mathl::logl(ay2l + ax2l + sl + sh) / ld(2.0), v);
    }
    sh = sh - ONE;
    two_sum(&mut sh, &mut sl);
    two_sum(&mut ax2l, &mut ay2l);
    // Briggs-Kahan algorithm (except we discard the final low term):
    two_sum(&mut sh, &mut ax2l);
    two_sum(&mut sl, &mut ay2l);
    let mut t = ax2l + sl;
    two_sum_f(&mut sh, &mut t);
    cl(crate::mathl::log1pl(ay2l + t + sh) / ld(2.0), v)
}

// ===========================================================================
// cpowl -- glibc's definition
// ===========================================================================

/// `x` to the power `y`: `cexpl(y * clogl(x))`, with the product formed as
/// `__mulxc3` forms it, which is how glibc defines `cpowl` -- and so its
/// answers, special values included. (FreeBSD's `s_cpowl.c` is Moshier's
/// polar formula, which answers differently near the special values.)
#[must_use]
pub fn cpowl(x: LdComplex, y: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let l = clogl(x);
    cexpl(mulxc3(y.re, y.im, l.re, l.im))
}

// ===========================================================================
// ccoshl, csinhl, ctanhl and their circular twins -- complex.rs's ccosh,
// csinh and ctanh (FreeBSD s_ccosh.c, s_csinh.c, s_ctanh.c), carried to the
// 80-bit format
//
// Copyright (c) 2005 Bruce D. Evans and Steven G. Kargl (ccosh, csinh)
// Copyright (c) 2011 David Schultz (ctanh)
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
//
// FreeBSD has no long double versions. The algorithms are the double ones;
// what changes is where they branch:
//
// - cosh x ~= e^|x| / 2 once the e^-|x| / 2 it drops is below half an ulp
//   of it: e^(-2|x|) < 2^-65, from |x| = 22.5 -- 23 here, where the double
//   code uses 22.
// - e^|x| overflows past exp_ovfl, 11356.52...; e^|x| * s overflows for
//   every s past cexp_ovfl, 22755.32... -- the double's 710 and 1455.
// - tanh x is +-1 to 64 bits, and the imaginary part of ctanh within its
//   ulp of 4 sin y cos y e^(-2|x|), from the same 23.
//
// The signs Annex G leaves open are glibc's, as in complex.rs.
// ===========================================================================

/// Past this, `cosh x` and `sinh x` are `e^|x| / 2` to 64 bits.
fn big_x() -> L {
    ld(23.0)
}

/// The largest `x` whose `e^x` is finite, 11356.523... (`cexpl`'s bound).
const EXP_OVFL: L = L::from_bits(0x400C, 0xB172_17F7_D1CF_79AC);

/// Past this, `e^x * s` overflows for every `s` there is, 22755.328...
const CEXP_OVFL: L = L::from_bits(0x400D, 0xB1C6_A857_3DE9_768C);

/// The hyperbolic cosine, `cosh(re) cos(im) + i sinh(re) sin(im)`.
#[must_use]
pub fn ccoshl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);
    let ax = x.abs();

    // The nearly-non-exceptional cases, where x and y are finite.
    if x.is_finite() && y.is_finite() {
        if y.is_zero() {
            return cl(crate::mathl::coshl(x), x * y);
        }
        if ax < big_x() {
            // |x| < 23: normal case
            let (s, c) = crate::mathl::sincosl(y);
            return cl(crate::mathl::coshl(x) * c, crate::mathl::sinhl(x) * s);
        }
        // |x| >= 23, so cosh(x) ~= exp(|x|)
        if ax < EXP_OVFL {
            // exp(|x|) won't overflow
            let h = crate::mathl::expl(ax) * ld(0.5);
            let (s, c) = crate::mathl::sincosl(y);
            return cl(h * c, h.copysign(x) * s);
        } else if ax < CEXP_OVFL {
            // scale to avoid overflow
            return ldexp_cexpl(ax, y, -1, ONE, ONE.copysign(x));
        }
        // the result always overflows -- cosh(x) cos(y) with cos(y)'s sign,
        // sinh(x) sin(y) with x's and sin(y)'s
        let (s, c) = crate::mathl::sincosl(y);
        return cl(overflowed(c), overflowed(signed_by(s, x)));
    }

    // cosh(+-0 +- I Inf) = dNaN + I 0, and cosh(+-0 +- I NaN) = d(NaN) +
    // I 0: glibc's always +0 (complex.rs's `ccosh` gives the whole case).
    if x.is_zero() {
        return cl(y - y, ZERO);
    }

    // cosh(+-Inf +- I 0) = +Inf + I (+-)(+-)0, and cosh(NaN +- I 0) =
    // d(NaN) +- I 0, keeping y's zero as glibc does.
    if y.is_zero() {
        if x.is_nan() {
            return cl(x * x, y);
        }
        return cl(x * x, ZERO.copysign(x) * y);
    }

    // cosh(x +- I Inf) = dNaN + I dNaN, raising invalid for finite nonzero
    // x; cosh(x + I NaN) = d(NaN) + I d(NaN).
    if x.is_finite() {
        return cl(y - y, x * (y - y));
    }

    // cosh(+-Inf + I NaN) = +Inf + I d(NaN); cosh(+-Inf +- I Inf) = +Inf +
    // I dNaN, raising invalid; cosh(+-Inf + I y) = +Inf cos(y) +- I Inf
    // sin(y).
    if x.is_infinite() {
        if !y.is_finite() {
            return cl(INF, x * (y - y));
        }
        let (s, c) = crate::mathl::sincosl(y);
        return cl(INF * c, x * s);
    }

    // cosh(NaN + I NaN) = d(NaN) + I d(NaN); cosh(NaN +- I Inf) = d(NaN) +
    // I d(NaN), raising invalid; cosh(NaN + I y) = d(NaN) + I d(NaN).
    cl((x * x) * (y - y), (x + x) * (y - y))
}

/// The cosine: `ccoshl(i z)`.
#[must_use]
pub fn ccosl(z: LdComplex) -> LdComplex {
    ccoshl(cl(z.im.negate(), z.re))
}

/// The hyperbolic sine, `sinh(re) cos(im) + i cosh(re) sin(im)`.
#[must_use]
pub fn csinhl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);
    let ax = x.abs();

    if x.is_finite() && y.is_finite() {
        if y.is_zero() {
            return cl(crate::mathl::sinhl(x), y);
        }
        if ax < big_x() {
            // |x| < 23: normal case
            let (s, c) = crate::mathl::sincosl(y);
            return cl(crate::mathl::sinhl(x) * c, crate::mathl::coshl(x) * s);
        }
        if ax < EXP_OVFL {
            let h = crate::mathl::expl(ax) * ld(0.5);
            let (s, c) = crate::mathl::sincosl(y);
            return cl(h.copysign(x) * c, h * s);
        } else if ax < CEXP_OVFL {
            return ldexp_cexpl(ax, y, -1, ONE.copysign(x), ONE);
        }
        // the result always overflows -- sinh(x) cos(y) with x's and cos(y)'s
        // signs, cosh(x) sin(y) with sin(y)'s
        let (s, c) = crate::mathl::sincosl(y);
        return cl(overflowed(signed_by(c, x)), overflowed(s));
    }

    // sinh(+-0 +- I Inf) = +-0 + I dNaN, raising invalid; sinh(+-0 +- I
    // NaN) = +-0 + I d(NaN): the zero keeps the argument's sign.
    if x.is_zero() {
        return cl(x, y - y);
    }

    // sinh(+-Inf +- I 0) = +-Inf + I +-0; sinh(NaN +- I 0) = d(NaN) + I +-0.
    if y.is_zero() {
        return cl(x + x, y);
    }

    // sinh(x +- I Inf) = dNaN + I dNaN, raising invalid for finite nonzero
    // x; sinh(x + I NaN) = d(NaN) + I d(NaN).
    if x.is_finite() {
        return cl(y - y, y - y);
    }

    // sinh(+-Inf + I NaN) = Inf + I d(NaN); sinh(+-Inf +- I Inf) = Inf + I
    // dNaN, raising invalid -- glibc's +Inf for both; sinh(+-Inf + I y) =
    // +-Inf cos(y) + I Inf sin(y).
    if x.is_infinite() {
        if !y.is_finite() {
            return cl(INF, y - y);
        }
        let (s, c) = crate::mathl::sincosl(y);
        return cl(x * c, INF * s);
    }

    // sinh(NaN1 + I NaN2), sinh(NaN +- I Inf) and sinh(NaN + I y): NaN +
    // I NaN.
    cl((x + x) * (y - y), (x * x) * (y - y))
}

/// The sine: `-i csinhl(i z)`.
#[must_use]
pub fn csinl(z: LdComplex) -> LdComplex {
    let w = csinhl(cl(z.im, z.re));
    cl(w.im, w.re)
}

/// The hyperbolic tangent, by Kahan's algorithm.
#[must_use]
pub fn ctanhl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);

    // ctanh(NaN +- I 0) = d(NaN) +- I 0; ctanh(NaN + I y) = d(NaN,y) + I
    // d(NaN,y) for y != 0; ctanh(+-Inf +- I Inf) = +-1 +- I 0;
    // ctanh(+-Inf + I y) = +-1 + I 0 sin(2y) for finite y. The last's sign
    // is unspecified; this case exists to avoid a spurious invalid when y is
    // infinite.
    if !x.is_finite() {
        if x.is_nan() {
            return cl(x + y, if y.is_zero() { y } else { x + y });
        }
        let s = if y.is_infinite() {
            y
        } else {
            let (s, c) = crate::mathl::sincosl(y);
            s * c
        };
        return cl(ONE.copysign(x), ZERO.copysign(s));
    }

    // ctanh(+-0 + i NaN) = +-0 + i NaN; ctanh(+-0 +- i Inf) = +-0 + i NaN;
    // ctanh(x + i NaN) = NaN + i NaN; ctanh(x +- i Inf) = NaN + i NaN.
    if !y.is_finite() {
        return cl(if x.is_zero() { x } else { y - y }, y - y);
    }

    // ctanh(+-huge +- I y) ~= +-1 +- I 2sin(2y)/exp(2x), using the
    // approximation sinh^2(huge) ~= exp(2*huge) / 4, rearranged to avoid a
    // spurious overflow.
    if x.abs() >= big_x() {
        let exp_mx = crate::mathl::expl(x.abs().negate());
        let (s, c) = crate::mathl::sincosl(y);
        return cl(ONE.copysign(x), ld(4.0) * s * c * exp_mx * exp_mx);
    }

    // Kahan's algorithm
    let t = crate::mathl::tanl(y);
    let beta = ONE + t * t; // = 1 / cos^2(y)
    let s = crate::mathl::sinhl(x);
    let rho = (ONE + s * s).sqrt(); // = cosh(x)
    let denom = ONE + beta * s * s;
    cl((beta * rho * s) / denom, t / denom)
}

/// The tangent: `-i ctanhl(i z)`.
#[must_use]
pub fn ctanl(z: LdComplex) -> LdComplex {
    let w = ctanhl(cl(z.im, z.re));
    cl(w.im, w.re)
}

// ===========================================================================
// casinhl, casinl, cacosl, cacoshl, catanhl, catanl -- FreeBSD catrigl.c
//
// Copyright (c) 2012 Stephen Montgomery-Smith <stephen@FreeBSD.ORG>
// Copyright (c) 2017 Mahdi Mokhtari <mmokhi@FreeBSD.org>
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
//
// Hull, Fairgrieve and Tang's algorithm, as complex.rs's `casinh` and the
// rest: see there for the whole argument. The signs Annex G leaves open are
// glibc's, as there.
// ===========================================================================

/// The constants of `catrigl.c`'s ld80 half.
mod kl {
    use super::{L, ld, p2l};
    pub(super) fn a_crossover() -> L {
        ld(10.0)
    }
    /// A `double` literal in FreeBSD's source, and so a `double`'s value.
    pub(super) fn b_crossover() -> L {
        ld(0.6417)
    }
    pub(super) const FOUR_SQRT_MIN: L = p2l(-8189);
    pub(super) const HALF_MAX: L = p2l(16383);
    pub(super) const QUARTER_SQRT_MAX: L = p2l(8189);
    /// `1 / LDBL_EPSILON`.
    pub(super) const RECIP_EPSILON: L = p2l(63);
    pub(super) const SQRT_MIN: L = p2l(-8191);
    /// `LDBL_EPSILON`.
    pub(super) const EPS: L = p2l(-63);
    pub(super) const M_E: L = L::from_bits(0x4000, 0xADF8_5458_A2BB_4A9B);
    pub(super) const M_LN2: L = L::from_bits(0x3FFE, 0xB172_17F7_D1CF_79AC);
    pub(super) const PIO2_HI: L = L::from_bits(0x3FFF, 0xC90F_DAA2_2168_C235);
    pub(super) const PIO2_LO: L = L::from_bits(0xBFBD, 0xECE6_75D1_FC8F_8CBB);
    /// `double` literals in FreeBSD's source ("misrounding them on i386 is
    /// harmless"), and so `double`s' values; thresholds, where an ulp moves
    /// nothing.
    pub(super) fn sqrt_3_epsilon() -> L {
        ld(5.703_162_734_357_59e-10)
    }
    pub(super) fn sqrt_6_epsilon() -> L {
        ld(8.065_490_087_349_327e-10)
    }
}

/// `pio2_hi + pio2_lo`, computed rather than folded, as FreeBSD's is.
fn pio2l() -> L {
    kl::PIO2_HI + core::hint::black_box(kl::PIO2_LO)
}

/// Raise inexact, as FreeBSD's `raise_inexact()` does.
fn raise_inexact() {
    let tiny = core::hint::black_box(f32::from_bits(0x0d80_0000)); // 2^-100
    let _ = core::hint::black_box(1.0_f32 + tiny);
}

/// `f(a, b, hypot(a, b)) = (hypot(a, b) - b) / 2`, without cancellation.
fn catrig_f(a: L, b: L, hypot_a_b: L) -> L {
    let two = ld(2.0);
    if b < ZERO {
        return (hypot_a_b - b) / two;
    }
    if b.is_zero() {
        return a / two;
    }
    a * a / (hypot_a_b + b) / two
}

/// What [`do_hard_work`] found, as `complex.rs`'s `HardWork`.
struct HardWork {
    rx: L,
    b: Option<L>,
    sqrt_a2my2: L,
    new_y: L,
}

/// All the hard work. `x` and `y` are non-negative and below
/// `RECIP_EPSILON`.
fn do_hard_work(x: L, y: L) -> HardWork {
    use crate::mathl::{hypotl, log1pl, logl};
    use kl::{EPS, FOUR_SQRT_MIN};
    let two = ld(2.0);
    let r = hypotl(x, y + ONE); // |z+I|
    let s = hypotl(x, y - ONE); // |z-I|

    // A = (|z+I| + |z-I|) / 2, mathematically >= 1.
    let mut a = (r + s) / two;
    if a < ONE {
        a = ONE;
    }

    let rx = if a < kl::a_crossover() {
        if y == ONE && x < EPS * EPS / ld(128.0) {
            x.sqrt()
        } else if x >= EPS * (y - ONE).abs() {
            let am1 = catrig_f(x, ONE + y, r) + catrig_f(x, ONE - y, s);
            log1pl(am1 + (am1 * (a + ONE)).sqrt())
        } else if y < ONE {
            x / ((ONE - y) * (ONE + y)).sqrt()
        } else {
            log1pl((y - ONE) + ((y - ONE) * (y + ONE)).sqrt())
        }
    } else {
        logl(a + (a * a - ONE).sqrt())
    };

    if y < FOUR_SQRT_MIN {
        return HardWork {
            rx,
            b: None,
            sqrt_a2my2: a * (two / EPS),
            new_y: y * (two / EPS),
        };
    }

    // B = (|z+I| - |z-I|) / 2 = y/A
    let b = y / a;
    if b <= kl::b_crossover() {
        return HardWork {
            rx,
            b: Some(b),
            sqrt_a2my2: ZERO,
            new_y: y,
        };
    }

    let (sqrt_a2my2, new_y) = if y == ONE && x < EPS / ld(128.0) {
        (x.sqrt() * ((a + y) / two).sqrt(), y)
    } else if x >= EPS * (y - ONE).abs() {
        let amy = catrig_f(x, y + ONE, r) + catrig_f(x, y - ONE, s);
        ((amy * (a + y)).sqrt(), y)
    } else if y > ONE {
        let four_over = ld(4.0) / EPS / EPS;
        (
            x * four_over * y / ((y + ONE) * (y - ONE)).sqrt(),
            y * four_over,
        )
    } else {
        (((ONE - y) * (ONE + y)).sqrt(), y)
    };
    HardWork {
        rx,
        b: None,
        sqrt_a2my2,
        new_y,
    }
}

/// `clogl` for `|z|` finite and larger than about `RECIP_EPSILON`.
fn clog_for_large_values(x: L, y: L) -> LdComplex {
    use crate::mathl::{atan2l, hypotl, logl};
    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }
    if ax > kl::HALF_MAX {
        return cl(logl(hypotl(x / kl::M_E, y / kl::M_E)) + ONE, atan2l(y, x));
    }
    if ax > kl::QUARTER_SQRT_MAX || ay < kl::SQRT_MIN {
        return cl(logl(hypotl(x, y)), atan2l(y, x));
    }
    cl(logl(ax * ax + ay * ay) / ld(2.0), atan2l(y, x))
}

/// The inverse hyperbolic sine, with branch cuts along the imaginary axis
/// outside `[-i, i]`.
#[must_use]
pub fn casinhl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return cl(x, y + y);
        }
        // glibc's copysign(Inf, x), as `casinh`.
        if y.is_infinite() {
            return cl(INF.copysign(x), x + x);
        }
        if y.is_zero() {
            return cl(x + x, y);
        }
        return cl(x + y, x + y);
    }

    if ax > kl::RECIP_EPSILON || ay > kl::RECIP_EPSILON {
        let w = if x.is_sign_negative() {
            clog_for_large_values(x.negate(), y.negate())
        } else {
            clog_for_large_values(x, y)
        };
        return cl((w.re + kl::M_LN2).copysign(x), w.im.copysign(y));
    }

    if x.is_zero() && y.is_zero() {
        return z;
    }

    raise_inexact();

    let q = kl::sqrt_6_epsilon() / ld(4.0);
    if ax < q && ay < q {
        return z;
    }

    let w = do_hard_work(ax, ay);
    let ry = match w.b {
        Some(b) => crate::mathl::asinl(b),
        None => crate::mathl::atan2l(w.new_y, w.sqrt_a2my2),
    };
    cl(w.rx.copysign(x), ry.copysign(y))
}

/// The inverse sine: `casinhl` with the parts exchanged on the way in and
/// out.
#[must_use]
pub fn casinl(z: LdComplex) -> LdComplex {
    let w = casinhl(cl(z.im, z.re));
    cl(w.im, w.re)
}

/// The inverse cosine, with branch cuts along the real axis outside
/// `[-1, 1]`.
#[must_use]
pub fn cacosl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    let (x, y) = (z.re, z.im);
    let (sx, sy) = (x.is_sign_negative(), y.is_sign_negative());
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return cl(y + y, INF.negate());
        }
        if y.is_infinite() {
            return cl(x + x, y.negate());
        }
        if x.is_zero() {
            return cl(pio2l(), y + y);
        }
        return cl(x + y, x + y);
    }

    if ax > kl::RECIP_EPSILON || ay > kl::RECIP_EPSILON {
        let w = clog_for_large_values(x, y);
        let rx = w.im.abs();
        let ry = w.re + kl::M_LN2;
        return cl(rx, if sy { ry } else { ry.negate() });
    }

    if x == ONE && y.is_zero() {
        return cl(ZERO, y.negate());
    }

    raise_inexact();

    let q = kl::sqrt_6_epsilon() / ld(4.0);
    if ax < q && ay < q {
        return cl(
            kl::PIO2_HI - (x - core::hint::black_box(kl::PIO2_LO)),
            y.negate(),
        );
    }

    let w = do_hard_work(ay, ax);
    let (ry, new_x, sqrt_a2mx2) = (w.rx, w.new_y, w.sqrt_a2my2);
    let rx = match w.b {
        Some(b) => crate::mathl::acosl(if sx { b.negate() } else { b }),
        None => crate::mathl::atan2l(sqrt_a2mx2, if sx { new_x.negate() } else { new_x }),
    };
    cl(rx, if sy { ry } else { ry.negate() })
}

/// The inverse hyperbolic cosine: `I*cacosl(z)` or `-I*cacosl(z)`,
/// whichever has a non-negative real part.
#[must_use]
pub fn cacoshl(z: LdComplex) -> LdComplex {
    let w = cacosl(z);
    let (rx, ry) = (w.re, w.im);
    if rx.is_nan() && ry.is_nan() {
        return cl(ry, rx);
    }
    if rx.is_nan() {
        return cl(ry.abs(), rx);
    }
    // glibc keeps cacos's pi/2 for +-0 + I*NaN, as `cacosh`.
    if ry.is_nan() {
        return cl(ry, rx);
    }
    cl(ry.abs(), rx.copysign(z.im))
}

/// `x*x + y*y`, or just `x*x` when `y*y` would underflow.
fn sum_squares(x: L, y: L) -> L {
    if y < kl::SQRT_MIN {
        return x * x;
    }
    x * x + y * y
}

/// `Re(1/(x + I*y)) = x/(x*x + y*y)` without the unwarranted underflow
/// `creal(1/z)` could give; FreeBSD's `real_part_reciprocal`, its exponent
/// arithmetic on the 15-bit field.
fn real_part_reciprocal(x: L, y: L) -> L {
    const BIAS: i32 = 16383; // LDBL_MAX_EXP - 1
    const CUTOFF: i32 = 64 / 2 + 1; // LDBL_MANT_DIG / 2 + 1
    let ix = i32::from(x.biased_exponent());
    let iy = i32::from(y.biased_exponent());
    if ix - iy >= CUTOFF || x.is_infinite() {
        return ONE / x;
    }
    if iy - ix >= CUTOFF {
        return x / y / y;
    }
    if ix <= BIAS + 16384 / 2 - CUTOFF {
        return x / (x * x + y * y);
    }
    // 2**(1 - ilogb(x)), as SET_LDBL_EXPSIGN(scale, 0x7fff - ix) sets it.
    let field = u16::try_from(0x7fff - ix).unwrap_or(0x3FFF);
    let scale = L::from_bits(field, 1 << 63);
    let (x, y) = (x * scale, y * scale);
    x / (x * x + y * y) * scale
}

/// The inverse hyperbolic tangent, with branch cuts along the real axis
/// outside `[-1, 1]`.
#[must_use]
pub fn catanhl(z: LdComplex) -> LdComplex {
    let _keep = KeepErrno::new();
    use crate::mathl::{atan2l, log1pl, logl};
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());
    let two = ld(2.0);
    let four = ld(4.0);

    // atanhl with errno: ERANGE at x = +-1, as glibc's catanhl.
    if y.is_zero() && ax <= ONE {
        return cl(crate::mathl::atanhl(x), y);
    }

    if x.is_zero() {
        return cl(x, crate::mathl::atanl(y));
    }

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return cl(ZERO.copysign(x), y + y);
        }
        if y.is_infinite() {
            return cl(ZERO.copysign(x), pio2l().copysign(y));
        }
        return cl(x + y, x + y);
    }

    if ax > kl::RECIP_EPSILON || ay > kl::RECIP_EPSILON {
        return cl(real_part_reciprocal(x, y), pio2l().copysign(y));
    }

    let q = kl::sqrt_3_epsilon() / two;
    if ax < q && ay < q {
        raise_inexact();
        return z;
    }

    let rx = if ax == ONE && ay < kl::EPS {
        (kl::M_LN2 - logl(ay)) / two
    } else {
        log1pl(four * ax / sum_squares(ax - ONE, ay)) / four
    };

    let ry = if ax == ONE {
        atan2l(two, ay.negate()) / two
    } else if ay < kl::EPS {
        atan2l(two * ay, (ONE - ax) * (ONE + ax)) / two
    } else {
        atan2l(two * ay, (ONE - ax) * (ONE + ax) - ay * ay) / two
    };

    cl(rx.copysign(x), ry.copysign(y))
}

/// The inverse tangent: `catanhl` with the parts exchanged on the way in
/// and out.
#[must_use]
pub fn catanl(z: LdComplex) -> LdComplex {
    let w = catanhl(cl(z.im, z.re));
    cl(w.im, w.re)
}

// ===========================================================================
// The C entry points: a Rust function on pointers per name, and its thunk
// (`crate::ld_c!`; see the module docs for the ABI)
// ===========================================================================

/// Shims and thunks for `long double complex f(long double complex)`.
macro_rules! export_cl_cl {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(z: *const LdComplex, out: *mut LdComplex) {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument and the thunk's result slot.
            unsafe { out.write($f(z.read())) }
        }
        crate::ld_c!(cl_cl $c => $shim);
    )* };
}

/// Shims and thunks for `long double f(long double complex)`: the argument
/// is where a `long double`'s would be, so the thunk is `l_l`'s.
macro_rules! export_l_cl {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(z: *const LdComplex, out: *mut L) {
            // SAFETY: as in `export_cl_cl!`.
            unsafe { out.write($f(z.read())) }
        }
        crate::ld_c!(l_l $c => $shim);
    )* };
}

export_cl_cl! {
    "conjl" __slate_ld_conjl conjl;
    "cprojl" __slate_ld_cprojl cprojl;
    "csqrtl" __slate_ld_csqrtl csqrtl;
    "cexpl" __slate_ld_cexpl cexpl;
    "clogl" __slate_ld_clogl clogl;
    "ccoshl" __slate_ld_ccoshl ccoshl;
    "ccosl" __slate_ld_ccosl ccosl;
    "csinhl" __slate_ld_csinhl csinhl;
    "csinl" __slate_ld_csinl csinl;
    "ctanhl" __slate_ld_ctanhl ctanhl;
    "ctanl" __slate_ld_ctanl ctanl;
    "casinhl" __slate_ld_casinhl casinhl;
    "casinl" __slate_ld_casinl casinl;
    "cacosl" __slate_ld_cacosl cacosl;
    "cacoshl" __slate_ld_cacoshl cacoshl;
    "catanhl" __slate_ld_catanhl catanhl;
    "catanl" __slate_ld_catanl catanl;
}

export_l_cl! {
    "creall" __slate_ld_creall creall;
    "cimagl" __slate_ld_cimagl cimagl;
    "cabsl" __slate_ld_cabsl cabsl;
    "cargl" __slate_ld_cargl cargl;
}

/// `cpowl(x, y)`, for its thunk.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_cpowl(
    x: *const LdComplex,
    y: *const LdComplex,
    out: *mut LdComplex,
) {
    // SAFETY: as in `export_cl_cl!`, two arguments.
    unsafe { out.write(cpowl(x.read(), y.read())) }
}
crate::ld_c!(cl_clcl "cpowl" => __slate_ld_cpowl);

/// `__mulxc3(a, b, c, d)`, for its thunk.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_mulxc3(
    a: *const L,
    b: *const L,
    c: *const L,
    d: *const L,
    out: *mut LdComplex,
) {
    // SAFETY: as in `export_cl_cl!`, four `long double` arguments.
    unsafe { out.write(mulxc3(a.read(), b.read(), c.read(), d.read())) }
}
crate::ld_c!(cl_llll "__mulxc3" => __slate_ld_mulxc3);

/// `__divxc3(a, b, c, d)`, for its thunk.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_divxc3(
    a: *const L,
    b: *const L,
    c: *const L,
    d: *const L,
    out: *mut LdComplex,
) {
    // SAFETY: as in `export_cl_cl!`, four `long double` arguments.
    unsafe { out.write(divxc3(a.read(), b.read(), c.read(), d.read())) }
}
crate::ld_c!(cl_llll "__divxc3" => __slate_ld_divxc3);

// ===========================================================================
// The licence the FreeBSD sources carry, which each section above names.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice, this list of conditions and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
// OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
// HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
// OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.
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

    /// glibc 2.39's answers (`posix/tools/oracle/complexl_harness.py`).
    const ORACLE: &str = include_str!("complexl_oracle.txt");

    /// glibc's answers in the three directed rounding modes, for each call of
    /// [`ORACLE`] whose answer there differs from its answer to nearest in
    /// kind -- an infinity, a NaN or a zero where to nearest it has none, or
    /// of the other sign -- or in `errno`: `<mode> <call>`, mode `down`, `up`
    /// or `zero` (`posix/tools/oracle/complexl_modes_harness.py`).
    const MODES_ORACLE: &str = include_str!("complexl_modes_oracle.txt");

    std::thread_local! {
        /// What [`ulps_allowed`] adds to a nonzero bound while
        /// [`every_call_answers_in_every_rounding_direction`] replays a
        /// directed mode; 0 otherwise.
        static DIRECTED_SLACK: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }

    /// How far a directed answer may be from glibc's answer *to nearest*,
    /// beyond the bound to nearest, where the call keeps its kind: each
    /// rounding goes the direction's way instead of to nearest, and Kahan's
    /// `ctan` and `ctanh` round at every step -- measured 4 at worst, there.
    const DIRECTED_DRIFT: u64 = 4;

    /// Every call of [`ORACLE`] again in each directed rounding mode: where
    /// [`MODES_ORACLE`] has the call, glibc's answer in that mode, within the
    /// bound to nearest and one ulp more, its infinities, NaNs and zeros
    /// exactly; elsewhere glibc's answer to nearest, within the bound and
    /// [`DIRECTED_DRIFT`] more. `errno` is the one to nearest where that is
    /// set, as `math.rs`'s replay has it (design-decisions section 1139).
    ///
    /// Before 2026-09-28 the double and float functions took their sines and
    /// cosines from the crate, whose argument reduction assumes rounding to
    /// nearest -- rounding upward `cexp(i pi/2)` had a real part of 2.3e-11
    /// -- and answered an overflow multiplied by a sine or cosine with a
    /// finite number: rounding downward, `cexpf(1e38 + i)` was 1.8e38, and
    /// `ccos(1 + 1000i)`'s imaginary part -DBL_MAX where the direction owes
    /// -inf. (The `long double`
    /// functions had the second bug, not the first.)
    #[test]
    fn every_call_answers_in_every_rounding_direction() {
        extended();
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
                let (want, slack) = match glibc {
                    Some(rhs) => {
                        let mut outs: Vec<&str> = rhs.split(' ').collect();
                        let near_errno = near.rsplit(' ').next().expect("an errno");
                        if near_errno != "0" {
                            *outs.last_mut().expect("an errno") = near_errno;
                        }
                        (format!("{lhs} = {}", outs.join(" ")), 1)
                    }
                    None => (line.to_owned(), DIRECTED_DRIFT),
                };
                n += 1;
                DIRECTED_SLACK.with(|s| s.set(slack));
                let result = {
                    let _r = Rounding::set(mode);
                    check_line(&want)
                };
                DIRECTED_SLACK.with(|s| s.set(0));
                if let Err(e) = result {
                    bad.push(format!(
                        "{mode_name} {want}\n    to nearest {near}\n    {e}"
                    ));
                }
            }
        }
        assert_eq!(
            listed.len(),
            directed.len(),
            "every directed line is a call of ORACLE's"
        );
        assert!(n > 70_000, "only {n} calls replayed");
        assert!(
            bad.is_empty(),
            "{} of {n} calls differ in a directed mode:\n{}",
            bad.len(),
            bad.iter().take(80).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// Rounds in `mode` -- both units, with the x87 at 64 bits -- until dropped, then to nearest
    /// again.
    struct Rounding;

    impl Rounding {
        fn set(mode: i32) -> Self {
            assert_eq!(crate::fenv::fesetround(mode), 0);
            Self
        }
    }

    impl Drop for Rounding {
        fn drop(&mut self) {
            crate::fenv::fesetround(crate::fenv::FE_TONEAREST);
        }
    }

    /// The fixes by name, in every directed mode: an overflow multiplied by
    /// a sine or cosine answers as the direction rounds the signed result.
    #[test]
    fn overflow_in_every_rounding_direction() {
        extended();
        let max = L::from_bits(0x7FFE, u64::MAX);
        let bits = |v: L| (v.sign_exp, v.significand);
        for (mode_name, mode) in [
            ("down", crate::fenv::FE_DOWNWARD),
            ("up", crate::fenv::FE_UPWARD),
            ("zero", crate::fenv::FE_TOWARDZERO),
        ] {
            let _r = Rounding::set(mode);
            let pos = if mode_name == "up" { INF } else { max };
            let neg = if mode_name == "down" {
                INF.negate()
            } else {
                max.negate()
            };
            // cexpl(LDBL_MAX + i): both parts overflow, positive.
            let w = cexpl(cl(max, ONE));
            assert_eq!(
                (bits(w.re), bits(w.im)),
                (bits(pos), bits(pos)),
                "cexpl {mode_name}"
            );
            // ccoshl(-12000 + 2i): cos(2) < 0, sin(2) > 0, x < 0 -- the scaled
            // branch, both parts negative.
            let w = ccoshl(cl(ld(-12000.0), ld(2.0)));
            assert_eq!(
                (bits(w.re), bits(w.im)),
                (bits(neg), bits(neg)),
                "ccoshl scaled {mode_name}"
            );
            // ccoshl(LDBL_MAX - 2i): the always-overflows branch; cos(-2) and
            // sin(-2) both negative.
            let w = ccoshl(cl(max, ld(-2.0)));
            assert_eq!(
                (bits(w.re), bits(w.im)),
                (bits(neg), bits(neg)),
                "ccoshl {mode_name}"
            );
        }
    }

    /// A value from the oracle's `SSSS:MMMMMMMMMMMMMMMM`.
    fn val(s: &str) -> L {
        let (se, m) = s.split_once(':').unwrap();
        L::from_bits(
            u16::from_str_radix(se, 16).unwrap(),
            u64::from_str_radix(m, 16).unwrap(),
        )
    }

    /// Units in the last place allowed, as `complex.rs`'s: 0 where the
    /// answer is exact by definition; elsewhere glibc's documented worst
    /// case for the `long double` function (`libm-test-ulps`, x86_64),
    /// FreeBSD's, and one for the real functions' own difference.
    fn ulps_allowed(name: &str) -> u64 {
        let base = match name.strip_suffix('l').unwrap_or(name) {
            "creal" | "cimag" | "conj" | "cproj" => 0,
            "cabs" | "carg" => 2,
            "cexp" | "csin" | "ccos" | "csinh" | "ccosh" | "csqrt" => 4,
            "clog" => 6,
            // Kahan's formula, which this uses, rounds at every step: the
            // worst row of the table is 5 ulp from the true value (mpmath, 200
            // bits), where glibc's is 2.
            "ctan" | "ctanh" => 8,
            "casin" | "cacos" | "casinh" | "cacosh" | "catan" | "catanh" => 8,
            other => panic!("no tolerance for {other}"),
        };
        if base == 0 {
            0
        } else {
            base + DIRECTED_SLACK.with(core::cell::Cell::get)
        }
    }

    /// A value's place on one ordered line of every finite 80-bit value --
    /// the subnormals, then each exponent's 2^63 significands -- so that the
    /// step across zero and across the subnormal boundary is one step like
    /// any other.
    fn line(v: L) -> i128 {
        let exp = i128::from(v.sign_exp & 0x7FFF);
        let m = i128::from(v.significand);
        let mag = if exp == 0 {
            m
        } else {
            exp * (1 << 63) + (m - (1 << 63))
        };
        if v.is_sign_negative() { -mag } else { mag }
    }

    /// One part, ours against glibc's, as `complex.rs`'s `same_part`: a NaN
    /// matches any NaN; an infinity or a zero must match exactly, sign
    /// included; anything else within `tol` ulps.
    fn same_part(ours: L, glibc: L, tol: u64) -> Result<(), String> {
        let bits = |v: L| (v.sign_exp, v.significand);
        if ours.is_nan() || glibc.is_nan() {
            return if ours.is_nan() && glibc.is_nan() {
                Ok(())
            } else {
                Err(format!("{:?}, glibc {:?}", bits(ours), bits(glibc)))
            };
        }
        if ours.is_infinite() || glibc.is_infinite() || (ours.is_zero() && glibc.is_zero()) {
            return if bits(ours) == bits(glibc) {
                Ok(())
            } else {
                Err(format!("{:?}, glibc {:?}", bits(ours), bits(glibc)))
            };
        }
        if (ours.is_zero() || glibc.is_zero())
            && ours.is_sign_negative() != glibc.is_sign_negative()
        {
            return Err(format!(
                "{:?}, glibc {:?}: the sign",
                bits(ours),
                bits(glibc)
            ));
        }
        let d = (line(ours) - line(glibc)).unsigned_abs();
        if d > u128::from(tol) {
            return Err(format!(
                "{:?}, glibc {:?}: {d} ulp",
                bits(ours),
                bits(glibc)
            ));
        }
        Ok(())
    }

    /// `cpowl`: special values as for the others; finite nonzero results as
    /// a relative error of the whole, which `clogl`'s ulps times `|y|` and
    /// then the exponential make (the table's `|x|, |y| <= 4`).
    fn cpow_close(ours: LdComplex, glibc: LdComplex) -> Result<(), String> {
        let special = |x: L| !x.is_finite() || x.is_zero();
        let f = |x: L| x.to_f64();
        if [ours.re, ours.im, glibc.re, glibc.im]
            .into_iter()
            .any(special)
        {
            for (o, g) in [(ours.re, glibc.re), (ours.im, glibc.im)] {
                if special(o) || special(g) {
                    same_part(o, g, 0)?;
                } else if ((f(o) - f(g)) / f(g)).abs() > 1e-15 {
                    return Err(format!("{:e}, glibc {:e}", f(o), f(g)));
                }
            }
            return Ok(());
        }
        let err = libm::hypot(f(ours.re) - f(glibc.re), f(ours.im) - f(glibc.im))
            / libm::hypot(f(glibc.re), f(glibc.im));
        if err > 1e-15 {
            return Err(format!("relative {err:e}"));
        }
        Ok(())
    }

    fn check_line(line: &str) -> Result<(), String> {
        let (lhs, rhs) = line.split_once(" = ").ok_or("no ` = `")?;
        let mut l = lhs.split(' ');
        let name = l.next().ok_or("no name")?;
        let args: Vec<L> = l.map(val).collect();
        let outs: Vec<&str> = rhs.split(' ').collect();
        let (vals, errno_s) = outs.split_at(outs.len() - 1);
        let errno_glibc: i32 = errno_s[0].parse().map_err(|_| "bad errno")?;
        crate::errno::set_errno(0);
        let z = cl(args[0], args[1]);
        let res = match name {
            "cabsl" | "cargl" | "creall" | "cimagl" => {
                let r = match name {
                    "cabsl" => cabsl(z),
                    "cargl" => cargl(z),
                    "creall" => creall(z),
                    _ => cimagl(z),
                };
                same_part(r, val(vals[0]), ulps_allowed(name))
            }
            "cpowl" => cpow_close(
                cpowl(z, cl(args[2], args[3])),
                cl(val(vals[0]), val(vals[1])),
            ),
            _ => {
                let func: fn(LdComplex) -> LdComplex = match name {
                    "csqrtl" => csqrtl,
                    "cexpl" => cexpl,
                    "clogl" => clogl,
                    "csinl" => csinl,
                    "ccosl" => ccosl,
                    "ctanl" => ctanl,
                    "csinhl" => csinhl,
                    "ccoshl" => ccoshl,
                    "ctanhl" => ctanhl,
                    "casinl" => casinl,
                    "cacosl" => cacosl,
                    "catanl" => catanl,
                    "casinhl" => casinhl,
                    "cacoshl" => cacoshl,
                    "catanhl" => catanhl,
                    "cprojl" => cprojl,
                    "conjl" => conjl,
                    other => return Err(format!("unknown {other}")),
                };
                let r = func(z);
                let tol = ulps_allowed(name);
                same_part(r.re, val(vals[0]), tol)
                    .map_err(|e| format!("re: {e}"))
                    .and_then(|()| {
                        same_part(r.im, val(vals[1]), tol).map_err(|e| format!("im: {e}"))
                    })
            }
        };
        res?;
        let errno_ours = crate::errno::get_errno();
        if errno_ours != errno_glibc {
            return Err(format!("errno {errno_ours}, glibc {errno_glibc}"));
        }
        Ok(())
    }

    /// Every one of glibc 2.39's answers for the 22 functions: its values,
    /// within each function's tolerance, its special values exactly, and
    /// its `errno`.
    #[test]
    fn the_long_double_complex_functions_answer_as_glibc_does() {
        extended();
        let mut bad = Vec::new();
        let mut per_name: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        let mut calls = 0;
        for line in ORACLE.lines() {
            calls += 1;
            if let Err(e) = check_line(line) {
                let name = line.split(' ').next().unwrap_or("?");
                *per_name.entry(name).or_default() += 1;
                bad.push(format!("{line}\n    {e}"));
            }
        }
        assert!(calls > 20_000, "only {calls} calls");
        assert!(
            bad.is_empty(),
            "{} of {calls} differ ({per_name:?}):\n{}",
            bad.len(),
            bad.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
        );
    }

    /// `__mulxc3` and `__divxc3` at Annex G's special points: an infinite
    /// operand times a finite nonzero one is infinite, a finite one over an
    /// infinite one is zero, over zero infinite.
    #[test]
    fn the_compilers_multiply_and_divide_follow_annex_g() {
        extended();
        let nan = L::NAN;
        let two = ld(2.0);
        let w = mulxc3(INF, nan, ONE, ONE);
        assert!(w.re.is_infinite() || w.im.is_infinite(), "{w:?}");
        let w = mulxc3(two, ld(3.0), ld(4.0), ld(5.0));
        assert_eq!((w.re.to_f64(), w.im.to_f64()), (-7.0, 22.0));
        let w = divxc3(ld(1.0), ld(1.0), INF, INF);
        assert!(w.re.is_zero() && w.im.is_zero(), "{w:?}");
        let w = divxc3(ld(1.0), ld(1.0), ZERO, ZERO);
        assert!(w.re.is_infinite() && w.im.is_infinite(), "{w:?}");
        let w = divxc3(ld(-7.0), ld(22.0), ld(4.0), ld(5.0));
        assert_eq!((w.re.to_f64(), w.im.to_f64()), (2.0, 3.0));
    }
}

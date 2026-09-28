//! `<complex.h>`: C99's complex functions (Annex G), for `double complex`
//! and `float complex`.
//!
//! # Where the code comes from
//!
//! FreeBSD's msun, release 14.1.0 (`lib/msun/src`), ported line for line:
//! `catrig.c` and `catrigf.c` (Stephen Montgomery-Smith) for the inverse
//! functions, `s_clog.c` (Bruce Evans) for `clog`, and David Schultz's,
//! Bruce Evans' and Steven Kargl's `s_csqrt.c`, `s_cexp.c`, `k_exp.c`,
//! `s_ccosh.c`, `s_csinh.c` and `s_ctanh.c` with their `float` twins, all
//! BSD-2-Clause (copyright notices below, at each function's section, as the
//! licence asks).
//!
//! **Why not musl's,** as `<math.h>` is: for `csqrt`, `cexp` and the
//! hyperbolic functions musl's *are* these (musl took them from FreeBSD), but
//! for `casin`, `cacos`, `casinh`, `cacosh` and `clog` musl has the textbook
//! formulas -- `clog(z) = log|z| + i arg z`, `casin(z) = -i log(iz +
//! sqrt(1 - z*z))` -- which lose most of their digits near the branch points
//! and near `|z| = 1`, and return NaN once `z*z` overflows (`|z|` above about
//! `1e154`). glibc, whose answers the tests replay, is accurate in all of
//! those places, and so is FreeBSD's `catrig.c`: it is Hull, Fairgrieve and
//! Tang's algorithm ("Implementing the complex arcsine and arccosine functions
//! using exception handling", ACM TOMS 23(3), 1997), within 4 ulp everywhere
//! by its author's testing.
//!
//! **`cpow` is glibc's definition**, `cexp(y * clog(x))` with the product
//! formed as a C compiler forms it (`__muldc3`'s Annex G rules), so that it
//! answers what glibc's does. FreeBSD's is a different formula (Moshier's),
//! and `cpow` amplifies every difference in `clog`.
//!
//! # The rest
//!
//! - **`errno`:** as glibc sets it, which is where its complex functions
//!   call its errno-setting real ones: `cabs` and `carg` (`hypot`, `atan2`),
//!   `cexp` (`ERANGE` when `e^re` underflows to zero -- and so `cpow`), and
//!   `catanh`/`catan` at `+-1` (`ERANGE`, from `log(0)`). Nothing else sets
//!   it. FreeBSD's code sets none; each place says what it adds.
//! - **Signs Annex G leaves open** are glibc's where FreeBSD chose
//!   differently; each such place says so.
//! - **The `long double` functions** (`cabsl` and the rest) are not here:
//!   a `long double` is x87's 80-bit format, passed in memory and returned on
//!   the x87 stack, and this library has no 80-bit arithmetic yet
//!   (known-issues.md -> `D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX`).
//!
//! # ABI
//!
//! A `double complex` is two `double`s, real part first, passed and returned
//! in `%xmm0` and `%xmm1`; a `float complex` is two `float`s packed into the
//! low half of one `%xmm` register. The SysV ABI classifies
//! [`Complex64`] and [`Complex32`] exactly so, which is why they are the
//! types here (compiler_rt.rs uses them for `__muldc3` for the same reason).

// `y - y` and `(b - b) / (b - b)` are Annex G's idiom, kept from the C: a NaN
// made *from* an operand -- NaN when it is infinite or NaN, raising invalid
// for an infinity as the standard asks -- where a constant NaN would raise
// nothing and lose the operand's payload.
#![allow(clippy::eq_op)]

use crate::compiler_rt::{__muldc3, __mulsc3, Complex32, Complex64};

const fn c64(re: f64, im: f64) -> Complex64 {
    Complex64 { re, im }
}

const fn c32(re: f32, im: f32) -> Complex32 {
    Complex32 { re, im }
}

/// `2^e`, exactly, for `e` in the normal range.
const fn p2(e: i32) -> f64 {
    f64::from_bits(((1023_i32.wrapping_add(e)) as u64) << 52)
}

/// `2^e` as a `float`, exactly, for `e` in the normal range.
const fn p2f(e: i32) -> f32 {
    f32::from_bits(((127_i32.wrapping_add(e)) as u32) << 23)
}

/// The high 32 bits of a double (FreeBSD's `GET_HIGH_WORD`).
fn hi(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// The low 32 bits of a double.
fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `x` with its high word replaced (FreeBSD's `SET_HIGH_WORD`).
fn with_hi(x: f64, h: u32) -> f64 {
    f64::from_bits((u64::from(h) << 32) | u64::from(lo(x)))
}

/// Raise FE_INEXACT, as FreeBSD's `raise_inexact()` does, by an addition the
/// compiler may not fold away.
fn raise_inexact() {
    let tiny = core::hint::black_box(p2f(-100));
    let _ = core::hint::black_box(1.0_f32 + tiny);
}

/// FreeBSD's `nan_mix(x, y)`: a NaN from either operand, quieted.
fn nan_mix(x: f64, y: f64) -> f64 {
    x + y
}

fn nan_mixf(x: f32, y: f32) -> f32 {
    x + y
}

// ===========================================================================
// The simple ones
// ===========================================================================

/// The real part.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn creal(z: Complex64) -> f64 {
    z.re
}

/// [`creal`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn crealf(z: Complex32) -> f32 {
    z.re
}

/// The imaginary part.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cimag(z: Complex64) -> f64 {
    z.im
}

/// [`cimag`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cimagf(z: Complex32) -> f32 {
    z.im
}

/// The complex conjugate: the imaginary part's sign flipped, a NaN's
/// included.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn conj(z: Complex64) -> Complex64 {
    c64(z.re, -z.im)
}

/// [`conj`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn conjf(z: Complex32) -> Complex32 {
    c32(z.re, -z.im)
}

/// The absolute value, `|z|`: `hypot`, and like it `ERANGE` when finite
/// parts overflow.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cabs(z: Complex64) -> f64 {
    crate::math::hypot(z.re, z.im)
}

/// [`cabs`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cabsf(z: Complex32) -> f32 {
    crate::math::hypotf(z.re, z.im)
}

/// The argument (phase angle), in `[-pi, pi]`: `atan2(im, re)`, and like it
/// `ERANGE` when a nonzero angle underflows.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn carg(z: Complex64) -> f64 {
    crate::math::atan2(z.im, z.re)
}

/// [`carg`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cargf(z: Complex32) -> f32 {
    crate::math::atan2f(z.im, z.re)
}

/// The projection onto the Riemann sphere: `z`, unless either part is
/// infinite -- then `inf + i0` with the imaginary part's sign, a NaN in the
/// other part notwithstanding.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cproj(z: Complex64) -> Complex64 {
    if !z.re.is_infinite() && !z.im.is_infinite() {
        z
    } else {
        c64(f64::INFINITY, 0.0_f64.copysign(z.im))
    }
}

/// [`cproj`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cprojf(z: Complex32) -> Complex32 {
    if !z.re.is_infinite() && !z.im.is_infinite() {
        z
    } else {
        c32(f32::INFINITY, 0.0_f32.copysign(z.im))
    }
}

// ===========================================================================
// csqrt -- FreeBSD s_csqrt.c / s_csqrtf.c
//
// Copyright (c) 2007 David Schultz <das@FreeBSD.ORG>
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// The square root, with its branch cut along the negative real axis:
/// the real part is never negative, and the imaginary part has the sign of
/// `z`'s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csqrt(z: Complex64) -> Complex64 {
    // For avoiding overflow for components >= DBL_MAX / (1 + sqrt(2)).
    const THRESH: f64 = f64::from_bits(0x7fda_8279_99fc_ef32);
    let (mut a, mut b) = (z.re, z.im);

    // Handle special cases.
    if a == 0.0 && b == 0.0 {
        return c64(0.0, b);
    }
    if b.is_infinite() {
        return c64(f64::INFINITY, b);
    }
    if a.is_nan() {
        let t = (b - b) / (b - b); // raise invalid if b is not a NaN
        return c64(a + t, a + t); // NaN + NaN i
    }
    if a.is_infinite() {
        // csqrt(inf + NaN i)  = inf +  NaN i
        // csqrt(inf + y i)    = inf +  0 i
        // csqrt(-inf + NaN i) = NaN +- inf i
        // csqrt(-inf + y i)   = 0   +  inf i
        return if a.is_sign_negative() {
            c64((b - b).abs(), a.copysign(b))
        } else {
            c64(a, (b - b).copysign(b))
        };
    }
    if b.is_nan() {
        let t = (a - a) / (a - a); // raise invalid
        return c64(b + t, b + t); // NaN + NaN i
    }

    // Scale to avoid overflow.
    let mut scale = if a.abs() >= THRESH || b.abs() >= THRESH {
        // Don't scale a or b if this might give (spurious) underflow. Then
        // the unscaled value is an equivalent infinitesimal (or 0).
        if a.abs() >= p2(-1020) {
            a *= 0.25;
        }
        if b.abs() >= p2(-1020) {
            b *= 0.25;
        }
        2.0
    } else {
        1.0
    };

    // Scale to reduce inaccuracies when both components are denormal.
    if a.abs() < p2(-1022) && b.abs() < p2(-1022) {
        a *= p2(54);
        b *= p2(54);
        scale = p2(-27);
    }

    // Algorithm 312, CACM vol 10, Oct 1967.
    if a >= 0.0 {
        let t = libm::sqrt((a + libm::hypot(a, b)) * 0.5);
        c64(scale * t, scale * b / (2.0 * t))
    } else {
        let t = libm::sqrt((-a + libm::hypot(a, b)) * 0.5);
        c64(scale * b.abs() / (2.0 * t), (scale * t).copysign(b))
    }
}

/// [`csqrt`] (float). The root is taken in double precision, which cannot
/// overflow for any float and rounds correctly in nearly all cases.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csqrtf(z: Complex32) -> Complex32 {
    let (a, b) = (z.re, z.im);

    if a == 0.0 && b == 0.0 {
        return c32(0.0, b);
    }
    if b.is_infinite() {
        return c32(f32::INFINITY, b);
    }
    if a.is_nan() {
        let t = (b - b) / (b - b);
        return c32(a + t, a + t);
    }
    if a.is_infinite() {
        return if a.is_sign_negative() {
            c32((b - b).abs(), a.copysign(b))
        } else {
            c32(a, (b - b).copysign(b))
        };
    }
    if b.is_nan() {
        let t = (a - a) / (a - a);
        return c32(b + t, b + t);
    }

    let (ad, bd) = (f64::from(a), f64::from(b));
    if a >= 0.0 {
        let t = libm::sqrt((ad + libm::hypot(ad, bd)) * 0.5);
        c32(t as f32, (bd / (2.0 * t)) as f32)
    } else {
        let t = libm::sqrt((-ad + libm::hypot(ad, bd)) * 0.5);
        c32((bd.abs() / (2.0 * t)) as f32, (t as f32).copysign(b))
    }
}

// ===========================================================================
// cexp and its scaling kernel -- FreeBSD s_cexp.c, k_exp.c and their float
// twins
//
// Copyright (c) 2011 David Schultz <das@FreeBSD.ORG>
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// `exp(x)` scaled into `[2^1023, 2^1024)`, with the power of two returned
/// separately, for `ln(DBL_MAX) <= x < ~1454.91` (`k_exp.c`'s
/// `__frexp_exp`).
fn frexp_exp(x: f64) -> (f64, i32) {
    const K: i32 = 1799; // constant for reduction
    const KLN2: f64 = 1_246.971_777_827_341_6; // K * ln2
    // exp(x) = exp(x - kln2) * 2**k, with k chosen to minimise
    // |exp(kln2) - 2**k|; the result's exponent is set to MAX_EXP so that it
    // can be multiplied by a tiny number without denormal losses.
    let exp_x = libm::exp(x - KLN2);
    let hx = hi(exp_x);
    let expt = ((hx >> 20) as i32)
        .wrapping_sub(0x3ff + 1023)
        .wrapping_add(K);
    (
        with_hi(exp_x, (hx & 0xf_ffff) | ((0x3ff + 1023) << 20)),
        expt,
    )
}

/// `errno` for an `e^re` that `cexp` computed, as glibc's `cexp` sets it.
///
/// glibc reaches its errno-setting `exp` only for a finite real part it has
/// scaled down to at most `(MAX_EXP - 1) * ln 2` (`s_cexp_template.c`), where
/// the one error `exp` can report is an underflow to zero -- so `ERANGE` then,
/// and never for an overflow, which glibc's scaling turns into a plain
/// infinite product. FreeBSD's code, which this module is, sets no `errno` at
/// all.
fn cexp_errno(x: f64, exp_x: f64) {
    if exp_x == 0.0 && x.is_finite() {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
}

/// [`cexp_errno`] (float).
fn cexpf_errno(x: f32, exp_x: f32) {
    if exp_x == 0.0 && x.is_finite() {
        crate::errno::set_errno(crate::errno::ERANGE);
    }
}

/// `2^e` from an exponent in the normal range, built from bits as
/// `INSERT_WORDS` does.
fn scale_of(e: i32) -> f64 {
    f64::from_bits((0x3ff_i64.wrapping_add(i64::from(e)) as u64) << 52)
}

/// `cexp(z) * 2^expt` for a large real part (`k_exp.c`'s `__ldexp_cexp`):
/// `expt` is small (0 or -1), and the caller has filtered out a real part so
/// large that overflow is inevitable.
fn ldexp_cexp(x: f64, y: f64, expt: i32) -> Complex64 {
    let (exp_x, ex_expt) = frexp_exp(x);
    let expt = expt.wrapping_add(ex_expt);
    // Arrange that scale1 * scale2 == 2**expt; either alone could be out of
    // range.
    let half = expt / 2;
    let scale1 = scale_of(half);
    let scale2 = scale_of(expt.wrapping_sub(half));
    let (s, c) = libm::sincos(y);
    c64(c * exp_x * scale1 * scale2, s * exp_x * scale1 * scale2)
}

/// The exponential, `e^z = e^re (cos im + i sin im)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cexp(z: Complex64) -> Complex64 {
    const EXP_OVFL: u32 = 0x4086_2e42; // high bits of MAX_EXP * ln2 ~= 710
    const CEXP_OVFL: u32 = 0x4096_b8e4; // (MAX_EXP - MIN_DENORM_EXP) * ln2
    let (x, y) = (z.re, z.im);
    let (hy, ly) = (hi(y) & 0x7fff_ffff, lo(y));

    // cexp(x + I 0) = exp(x) + I 0
    if (hy | ly) == 0 {
        let exp_x = libm::exp(x);
        cexp_errno(x, exp_x);
        return c64(exp_x, y);
    }
    let (hx, lx) = (hi(x), lo(x));
    // cexp(0 + I y) = cos(y) + I sin(y)
    if ((hx & 0x7fff_ffff) | lx) == 0 {
        let (s, c) = libm::sincos(y);
        return c64(c, s);
    }

    if hy >= 0x7ff0_0000 {
        return if lx != 0 || (hx & 0x7fff_ffff) != 0x7ff0_0000 {
            // cexp(finite|NaN +- I Inf|NaN) = NaN + I NaN
            c64(y - y, y - y)
        } else if hx & 0x8000_0000 != 0 {
            // cexp(-Inf +- I Inf|NaN) = 0 +- I 0. Annex G leaves the zeros'
            // signs open; FreeBSD answers +0 + I 0, glibc gives the imaginary
            // zero the sign of y, a NaN's included, and so does this.
            c64(0.0, 0.0_f64.copysign(y))
        } else {
            // cexp(+Inf +- I Inf|NaN) = Inf + I NaN
            c64(x, y - y)
        };
    }

    if (EXP_OVFL..=CEXP_OVFL).contains(&hx) {
        // x is between 709.7 and 1454.3, so we must scale to avoid overflow
        // in exp(x).
        ldexp_cexp(x, y, 0)
    } else {
        // x < EXP_OVFL (the common case: exp(x) cannot overflow); or
        // x > CEXP_OVFL, where exp(x) * s overflows for every s > 0; or x is
        // +-Inf or NaN.
        let exp_x = libm::exp(x);
        cexp_errno(x, exp_x);
        let (s, c) = libm::sincos(y);
        c64(exp_x * c, exp_x * s)
    }
}

fn frexp_expf(x: f32) -> (f32, i32) {
    const K: i32 = 235;
    const KLN2: f32 = 162.889_59; // K * ln2
    let exp_x = libm::expf(x - KLN2);
    let hx = exp_x.to_bits();
    let expt = ((hx >> 23) as i32).wrapping_sub(0x7f + 127).wrapping_add(K);
    (
        f32::from_bits((hx & 0x7f_ffff) | ((0x7f + 127) << 23)),
        expt,
    )
}

fn scale_off(e: i32) -> f32 {
    f32::from_bits((0x7f_i32.wrapping_add(e) as u32) << 23)
}

fn ldexp_cexpf(x: f32, y: f32, expt: i32) -> Complex32 {
    let (exp_x, ex_expt) = frexp_expf(x);
    let expt = expt.wrapping_add(ex_expt);
    let half = expt / 2;
    let scale1 = scale_off(half);
    let scale2 = scale_off(expt.wrapping_sub(half));
    let (s, c) = libm::sincosf(y);
    c32(c * exp_x * scale1 * scale2, s * exp_x * scale1 * scale2)
}

/// [`cexp`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cexpf(z: Complex32) -> Complex32 {
    const EXP_OVFL: u32 = 0x42b1_7218; // MAX_EXP * ln2 ~= 88.722839355
    const CEXP_OVFL: u32 = 0x4340_0074; // (MAX_EXP - MIN_DENORM_EXP) * ln2
    let (x, y) = (z.re, z.im);
    let hy = y.to_bits() & 0x7fff_ffff;

    if hy == 0 {
        let exp_x = libm::expf(x);
        cexpf_errno(x, exp_x);
        return c32(exp_x, y);
    }
    let hx = x.to_bits();
    // cexp(+-0 + I y) = cos(y) + I sin(y)
    if x == 0.0 {
        let (s, c) = libm::sincosf(y);
        return c32(c, s);
    }

    if hy >= 0x7f80_0000 {
        return if (hx & 0x7fff_ffff) != 0x7f80_0000 {
            c32(y - y, y - y)
        } else if hx & 0x8000_0000 != 0 {
            c32(0.0, 0.0_f32.copysign(y))
        } else {
            c32(x, y - y)
        };
    }

    if (EXP_OVFL..=CEXP_OVFL).contains(&hx) {
        ldexp_cexpf(x, y, 0)
    } else {
        let exp_x = libm::expf(x);
        cexpf_errno(x, exp_x);
        let (s, c) = libm::sincosf(y);
        c32(exp_x * c, exp_x * s)
    }
}

// ===========================================================================
// clog -- FreeBSD s_clog.c / s_clogf.c
//
// Copyright (c) 2013 Bruce D. Evans
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
// ===========================================================================

/// FreeBSD's `_2sum(a, b)`: `a + b` exactly, as the rounded sum in `a` and
/// the rounding error in `b`, whatever their magnitudes.
fn two_sum(a: &mut f64, b: &mut f64) {
    let w = *a + *b;
    let s = w - *a;
    *b = (*a - (w - s)) + (*b - s);
    *a = w;
}

/// FreeBSD's `_2sumF(a, b)`: the same for `|a| >= |b|` (or `a == 0`),
/// cheaper.
fn two_sum_f(a: &mut f64, b: &mut f64) {
    let w = *a + *b;
    // `b = (a - w) + b` in the C; an IEEE sum is the same either way round.
    *b += *a - w;
    *a = w;
}

fn two_sumf(a: &mut f32, b: &mut f32) {
    let w = *a + *b;
    let s = w - *a;
    *b = (*a - (w - s)) + (*b - s);
    *a = w;
}

fn two_sum_ff(a: &mut f32, b: &mut f32) {
    let w = *a + *b;
    *b += *a - w;
    *a = w;
}

/// The natural logarithm, with its branch cut along the negative real axis:
/// `log|z| + i arg z`, computed so that `|z|` near 1 keeps its digits.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clog(z: Complex64) -> Complex64 {
    const MANT_DIG: i32 = 53;
    const MAX_EXP: i32 = 1024;
    const MIN_EXP: i32 = -1021;
    const LN2_HI: f64 = 6.931_471_805_582_987e-1; // 0x162e42fefa0000.0p-53
    const LN2_LO: f64 = 1.646_594_958_289_708_2e-12; // 0x1cf79abc9e3b3a.0p-92

    let (x, y) = (z.re, z.im);
    let v = libm::atan2(y, x);

    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }

    let kx = ((hi(ax) >> 20) as i32).wrapping_sub(1023);
    let ky = ((hi(ay) >> 20) as i32).wrapping_sub(1023);

    // Handle NaNs and Infs using the general formula.
    if kx == MAX_EXP || ky == MAX_EXP {
        return c64(libm::log(libm::hypot(x, y)), v);
    }

    // Avoid spurious underflow, and reduce inaccuracies when ax is 1.
    if ax == 1.0 {
        if ky < (MIN_EXP - 1) / 2 {
            return c64((ay / 2.0) * ay, v);
        }
        return c64(libm::log1p(ay * ay) / 2.0, v);
    }

    // Avoid underflow when ax is not small. Also handle zero args.
    if kx.wrapping_sub(ky) > MANT_DIG || ay == 0.0 {
        return c64(libm::log(ax), v);
    }

    // Avoid overflow.
    if kx >= MAX_EXP - 1 {
        return c64(
            libm::log(libm::hypot(x * p2(-1022), y * p2(-1022)))
                + f64::from(MAX_EXP - 2) * LN2_LO
                + f64::from(MAX_EXP - 2) * LN2_HI,
            v,
        );
    }
    if kx >= (MAX_EXP - 1) / 2 {
        return c64(libm::log(libm::hypot(x, y)), v);
    }

    // Reduce inaccuracies and avoid underflow when ax is denormal.
    if kx <= MIN_EXP - 2 {
        return c64(
            libm::log(libm::hypot(x * p2(1023), y * p2(1023)))
                + f64::from(MIN_EXP - 2) * LN2_LO
                + f64::from(MIN_EXP - 2) * LN2_HI,
            v,
        );
    }

    // Avoid remaining underflows (when ax is small but not denormal).
    if ky < (MIN_EXP - 1) / 2 + MANT_DIG {
        return c64(libm::log(libm::hypot(x, y)), v);
    }

    // Calculate ax*ax and ay*ay exactly using Dekker's algorithm.
    let t = ax * (p2(27) + 1.0);
    let axh = (ax - t) + t;
    let axl = ax - axh;
    let ax2h = ax * ax;
    let mut ax2l = axh * axh - ax2h + 2.0 * axh * axl + axl * axl;
    let t = ay * (p2(27) + 1.0);
    let ayh = (ay - t) + t;
    let ayl = ay - ayh;
    let ay2h = ay * ay;
    let mut ay2l = ayh * ayh - ay2h + 2.0 * ayh * ayl + ayl * ayl;

    // When log|z| is far from 1, the sum of the squares need not be very
    // accurate, since log reduces inaccuracies; near 1, subtract 1 in
    // doubled precision and use log1p, which keeps the bits a plain log
    // would cancel (FreeBSD's comment has the full argument).
    let mut sh = ax2h;
    let mut sl = ay2h;
    two_sum_f(&mut sh, &mut sl);
    if !(0.5..3.0).contains(&sh) {
        return c64(libm::log(ay2l + ax2l + sl + sh) / 2.0, v);
    }
    sh -= 1.0;
    two_sum(&mut sh, &mut sl);
    two_sum(&mut ax2l, &mut ay2l);
    // Briggs-Kahan algorithm (except we discard the final low term):
    two_sum(&mut sh, &mut ax2l);
    two_sum(&mut sl, &mut ay2l);
    let mut t = ax2l + sl;
    two_sum_f(&mut sh, &mut t);
    c64(libm::log1p(ay2l + t + sh) / 2.0, v)
}

/// [`clog`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn clogf(z: Complex32) -> Complex32 {
    const MANT_DIG: i32 = 24;
    const MAX_EXP: i32 = 128;
    const MIN_EXP: i32 = -125;
    const LN2F_HI: f32 = 6.931_457_5e-1; // 0xb17200.0p-24
    const LN2F_LO: f32 = 1.428_606_8e-6; // 0xbfbe8e.0p-43

    let (x, y) = (z.re, z.im);
    let v = libm::atan2f(y, x);

    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }

    let hax = ax.to_bits();
    let kx = ((hax >> 23) as i32).wrapping_sub(127);
    let hay = ay.to_bits();
    let ky = ((hay >> 23) as i32).wrapping_sub(127);

    if kx == MAX_EXP || ky == MAX_EXP {
        return c32(libm::logf(libm::hypotf(x, y)), v);
    }

    if hax == 0x3f80_0000 {
        if ky < (MIN_EXP - 1) / 2 {
            return c32((ay / 2.0) * ay, v);
        }
        return c32(libm::log1pf(ay * ay) / 2.0, v);
    }

    if kx.wrapping_sub(ky) > MANT_DIG || hay == 0 {
        return c32(libm::logf(ax), v);
    }

    if kx >= MAX_EXP - 1 {
        return c32(
            libm::logf(libm::hypotf(x * p2f(-126), y * p2f(-126)))
                + (MAX_EXP - 2) as f32 * LN2F_LO
                + (MAX_EXP - 2) as f32 * LN2F_HI,
            v,
        );
    }
    if kx >= (MAX_EXP - 1) / 2 {
        return c32(libm::logf(libm::hypotf(x, y)), v);
    }

    if kx <= MIN_EXP - 2 {
        return c32(
            libm::logf(libm::hypotf(x * p2f(127), y * p2f(127)))
                + (MIN_EXP - 2) as f32 * LN2F_LO
                + (MIN_EXP - 2) as f32 * LN2F_HI,
            v,
        );
    }

    if ky < (MIN_EXP - 1) / 2 + MANT_DIG {
        return c32(libm::logf(libm::hypotf(x, y)), v);
    }

    let t = ax * (p2f(12) + 1.0);
    let axh = (ax - t) + t;
    let axl = ax - axh;
    let ax2h = ax * ax;
    let mut ax2l = axh * axh - ax2h + 2.0 * axh * axl + axl * axl;
    let t = ay * (p2f(12) + 1.0);
    let ayh = (ay - t) + t;
    let ayl = ay - ayh;
    let ay2h = ay * ay;
    let mut ay2l = ayh * ayh - ay2h + 2.0 * ayh * ayl + ayl * ayl;

    let mut sh = ax2h;
    let mut sl = ay2h;
    two_sum_ff(&mut sh, &mut sl);
    if !(0.5..3.0).contains(&sh) {
        return c32(libm::logf(ay2l + ax2l + sl + sh) / 2.0, v);
    }
    sh -= 1.0;
    two_sumf(&mut sh, &mut sl);
    two_sumf(&mut ax2l, &mut ay2l);
    two_sumf(&mut sh, &mut ax2l);
    two_sumf(&mut sl, &mut ay2l);
    let mut t = ax2l + sl;
    two_sum_ff(&mut sh, &mut t);
    c32(libm::log1pf(ay2l + t + sh) / 2.0, v)
}

// ===========================================================================
// cpow -- glibc's definition
// ===========================================================================

/// `x` to the power `y`: `cexp(y * clog(x))`, glibc's definition, with the
/// product formed as a C compiler forms `y * clog(x)` -- `__muldc3`, whose
/// Annex G rules turn an infinite factor into an infinite product rather
/// than NaN. Its branch cut is `clog`'s.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpow(x: Complex64, y: Complex64) -> Complex64 {
    let l = clog(x);
    cexp(__muldc3(y.re, y.im, l.re, l.im))
}

/// [`cpow`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cpowf(x: Complex32, y: Complex32) -> Complex32 {
    let l = clogf(x);
    cexpf(__mulsc3(y.re, y.im, l.re, l.im))
}

// ===========================================================================
// ccosh, csinh, ctanh and their circular twins -- FreeBSD s_ccosh.c,
// s_csinh.c, s_ctanh.c and their float twins
//
// Copyright (c) 2005 Bruce D. Evans and Steven G. Kargl (ccosh, csinh)
// Copyright (c) 2011 David Schultz (ctanh)
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
//
// The signs Annex G leaves unspecified must still satisfy
// cosh(conj(z)) == conj(cosh(z)) and cosh(-z) == cosh(z) (and the odd
// analogues for sinh and tanh); FreeBSD's comments name each choice.
// ===========================================================================

/// The hyperbolic cosine, `cosh(re) cos(im) + i sinh(re) sin(im)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ccosh(z: Complex64) -> Complex64 {
    const HUGE: f64 = p2(1023);
    let (x, y) = (z.re, z.im);
    let (hx, lx) = (hi(x), lo(x));
    let (hy, ly) = (hi(y), lo(y));
    let ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;

    // The nearly-non-exceptional cases, where x and y are finite.
    if ix < 0x7ff0_0000 && iy < 0x7ff0_0000 {
        if (iy | ly) == 0 {
            return c64(libm::cosh(x), x * y);
        }
        if ix < 0x4036_0000 {
            // |x| < 22: normal case
            return c64(libm::cosh(x) * libm::cos(y), libm::sinh(x) * libm::sin(y));
        }
        // |x| >= 22, so cosh(x) ~= exp(|x|)
        if ix < 0x4086_2e42 {
            // |x| < 710: exp(|x|) won't overflow
            let h = libm::exp(x.abs()) * 0.5;
            return c64(h * libm::cos(y), h.copysign(x) * libm::sin(y));
        } else if ix < 0x4096_bbaa {
            // |x| < 1455: scale to avoid overflow
            let w = ldexp_cexp(x.abs(), y, -1);
            return c64(w.re, w.im * 1.0_f64.copysign(x));
        }
        // |x| >= 1455: the result always overflows
        let h = HUGE * x;
        return c64(h * h * libm::cos(y), h * libm::sin(y));
    }

    // cosh(+-0 +- I Inf) = dNaN + I 0, and cosh(+-0 +- I NaN) = d(NaN) +
    // I 0. Annex G leaves the zero's sign open; FreeBSD gives it the product
    // of the arguments' signs, glibc (`s_ccosh_template.c`) always +0, and so
    // does this. Raise invalid for the Inf.
    if (ix | lx) == 0 {
        return c64(y - y, 0.0);
    }

    // cosh(+-Inf +- I 0) = +Inf + I (+-)(+-)0, and cosh(NaN +- I 0) =
    // d(NaN) +- I 0: for a NaN real part glibc keeps y's own zero, where
    // FreeBSD multiplies in the NaN's sign; this keeps y's.
    if (iy | ly) == 0 {
        if x.is_nan() {
            return c64(x * x, y);
        }
        return c64(x * x, 0.0_f64.copysign(x) * y);
    }

    // cosh(x +- I Inf) = dNaN + I dNaN, raising invalid for finite nonzero
    // x; cosh(x + I NaN) = d(NaN) + I d(NaN).
    if ix < 0x7ff0_0000 {
        return c64(y - y, x * (y - y));
    }

    // cosh(+-Inf + I NaN) = +Inf + I d(NaN); cosh(+-Inf +- I Inf) = +Inf +
    // I dNaN, raising invalid; cosh(+-Inf + I y) = +Inf cos(y) +- I Inf
    // sin(y).
    if ix == 0x7ff0_0000 && lx == 0 {
        if iy >= 0x7ff0_0000 {
            return c64(f64::INFINITY, x * (y - y));
        }
        return c64(f64::INFINITY * libm::cos(y), x * libm::sin(y));
    }

    // cosh(NaN + I NaN) = d(NaN) + I d(NaN); cosh(NaN +- I Inf) = d(NaN) +
    // I d(NaN), raising invalid; cosh(NaN + I y) = d(NaN) + I d(NaN).
    c64((x * x) * (y - y), (x + x) * (y - y))
}

/// The cosine: `ccosh(i z)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ccos(z: Complex64) -> Complex64 {
    ccosh(c64(-z.im, z.re))
}

/// The hyperbolic sine, `sinh(re) cos(im) + i cosh(re) sin(im)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csinh(z: Complex64) -> Complex64 {
    const HUGE: f64 = p2(1023);
    let (x, y) = (z.re, z.im);
    let (hx, lx) = (hi(x), lo(x));
    let (hy, ly) = (hi(y), lo(y));
    let ix = hx & 0x7fff_ffff;
    let iy = hy & 0x7fff_ffff;

    if ix < 0x7ff0_0000 && iy < 0x7ff0_0000 {
        if (iy | ly) == 0 {
            return c64(libm::sinh(x), y);
        }
        if ix < 0x4036_0000 {
            // |x| < 22: normal case
            return c64(libm::sinh(x) * libm::cos(y), libm::cosh(x) * libm::sin(y));
        }
        if ix < 0x4086_2e42 {
            let h = libm::exp(x.abs()) * 0.5;
            return c64(h.copysign(x) * libm::cos(y), h * libm::sin(y));
        } else if ix < 0x4096_bbaa {
            let w = ldexp_cexp(x.abs(), y, -1);
            return c64(w.re * 1.0_f64.copysign(x), w.im);
        }
        let h = HUGE * x;
        return c64(h * libm::cos(y), h * h * libm::sin(y));
    }

    // sinh(+-0 +- I Inf) = +-0 + I dNaN, raising invalid; sinh(+-0 +- I
    // NaN) = +-0 + I d(NaN): the zero keeps the argument's sign.
    if (ix | lx) == 0 {
        return c64(x, y - y);
    }

    // sinh(+-Inf +- I 0) = +-Inf + I +-0; sinh(NaN +- I 0) = d(NaN) + I +-0.
    if (iy | ly) == 0 {
        return c64(x + x, y);
    }

    // sinh(x +- I Inf) = dNaN + I dNaN, raising invalid for finite nonzero
    // x; sinh(x + I NaN) = d(NaN) + I d(NaN).
    if ix < 0x7ff0_0000 {
        return c64(y - y, y - y);
    }

    // sinh(+-Inf + I NaN) = Inf + I d(NaN); sinh(+-Inf +- I Inf) = Inf + I
    // dNaN, raising invalid; sinh(+-Inf + I y) = +-Inf cos(y) + I Inf
    // sin(y). The first two infinities' signs are open in Annex G: FreeBSD
    // keeps x's, glibc (`s_csinh_template.c`) answers +Inf, and so does
    // this.
    if ix == 0x7ff0_0000 && lx == 0 {
        if iy >= 0x7ff0_0000 {
            return c64(f64::INFINITY, y - y);
        }
        return c64(x * libm::cos(y), f64::INFINITY * libm::sin(y));
    }

    // sinh(NaN1 + I NaN2), sinh(NaN +- I Inf) and sinh(NaN + I y): NaN +
    // I NaN.
    c64((x + x) * (y - y), (x * x) * (y - y))
}

/// The sine: `-i csinh(i z)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csin(z: Complex64) -> Complex64 {
    let w = csinh(c64(z.im, z.re));
    c64(w.im, w.re)
}

/// The hyperbolic tangent, by Kahan's algorithm.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctanh(z: Complex64) -> Complex64 {
    let (mut x, y) = (z.re, z.im);
    let hx = hi(x);
    let ix = hx & 0x7fff_ffff;

    // ctanh(NaN +- I 0) = d(NaN) +- I 0; ctanh(NaN + I y) = d(NaN,y) + I
    // d(NaN,y) for y != 0; ctanh(+-Inf +- I Inf) = +-1 +- I 0;
    // ctanh(+-Inf + I y) = +-1 + I 0 sin(2y) for finite y. The last's sign
    // is unspecified; this case exists to avoid a spurious invalid when y is
    // infinite.
    if ix >= 0x7ff0_0000 {
        if ((ix & 0xf_ffff) | lo(x)) != 0 {
            // x is NaN
            return c64(nan_mix(x, y), if y == 0.0 { y } else { nan_mix(x, y) });
        }
        x = with_hi(x, hx.wrapping_sub(0x4000_0000)); // x = copysign(1, x)
        let s = if y.is_infinite() {
            y
        } else {
            libm::sin(y) * libm::cos(y)
        };
        return c64(x, 0.0_f64.copysign(s));
    }

    // ctanh(+-0 + i NaN) = +-0 + i NaN; ctanh(+-0 +- i Inf) = +-0 + i NaN;
    // ctanh(x + i NaN) = NaN + i NaN; ctanh(x +- i Inf) = NaN + i NaN.
    if !y.is_finite() {
        return c64(if x == 0.0 { x } else { y - y }, y - y);
    }

    // ctanh(+-huge +- I y) ~= +-1 +- I 2sin(2y)/exp(2x), using the
    // approximation sinh^2(huge) ~= exp(2*huge) / 4, rearranged to avoid a
    // spurious overflow.
    if ix >= 0x4036_0000 {
        // |x| >= 22
        let exp_mx = libm::exp(-x.abs());
        return c64(
            1.0_f64.copysign(x),
            4.0 * libm::sin(y) * libm::cos(y) * exp_mx * exp_mx,
        );
    }

    // Kahan's algorithm
    let t = libm::tan(y);
    let beta = 1.0 + t * t; // = 1 / cos^2(y)
    let s = libm::sinh(x);
    let rho = libm::sqrt(1.0 + s * s); // = cosh(x)
    let denom = 1.0 + beta * s * s;
    c64((beta * rho * s) / denom, t / denom)
}

/// The tangent: `-i ctanh(i z)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctan(z: Complex64) -> Complex64 {
    let w = ctanh(c64(z.im, z.re));
    c64(w.im, w.re)
}

/// [`ccosh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ccoshf(z: Complex32) -> Complex32 {
    const HUGE: f32 = p2f(127);
    let (x, y) = (z.re, z.im);
    let ix = x.to_bits() & 0x7fff_ffff;
    let iy = y.to_bits() & 0x7fff_ffff;

    if ix < 0x7f80_0000 && iy < 0x7f80_0000 {
        if iy == 0 {
            return c32(libm::coshf(x), x * y);
        }
        if ix < 0x4110_0000 {
            // |x| < 9: normal case
            return c32(
                libm::coshf(x) * libm::cosf(y),
                libm::sinhf(x) * libm::sinf(y),
            );
        }
        if ix < 0x42b1_7218 {
            // |x| < 88.7: expf(|x|) won't overflow
            let h = libm::expf(x.abs()) * 0.5;
            return c32(h * libm::cosf(y), h.copysign(x) * libm::sinf(y));
        } else if ix < 0x4340_b1e7 {
            // |x| < 192.7: scale to avoid overflow
            let w = ldexp_cexpf(x.abs(), y, -1);
            return c32(w.re, w.im * 1.0_f32.copysign(x));
        }
        let h = HUGE * x;
        return c32(h * h * libm::cosf(y), h * libm::sinf(y));
    }

    if ix == 0 {
        return c32(y - y, 0.0);
    }
    if iy == 0 {
        if x.is_nan() {
            return c32(x * x, y);
        }
        return c32(x * x, 0.0_f32.copysign(x) * y);
    }
    if ix < 0x7f80_0000 {
        return c32(y - y, x * (y - y));
    }
    if ix == 0x7f80_0000 {
        if iy >= 0x7f80_0000 {
            return c32(f32::INFINITY, x * (y - y));
        }
        return c32(f32::INFINITY * libm::cosf(y), x * libm::sinf(y));
    }
    c32((x * x) * (y - y), (x + x) * (y - y))
}

/// [`ccos`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ccosf(z: Complex32) -> Complex32 {
    ccoshf(c32(-z.im, z.re))
}

/// [`csinh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csinhf(z: Complex32) -> Complex32 {
    const HUGE: f32 = p2f(127);
    let (x, y) = (z.re, z.im);
    let ix = x.to_bits() & 0x7fff_ffff;
    let iy = y.to_bits() & 0x7fff_ffff;

    if ix < 0x7f80_0000 && iy < 0x7f80_0000 {
        if iy == 0 {
            return c32(libm::sinhf(x), y);
        }
        if ix < 0x4110_0000 {
            return c32(
                libm::sinhf(x) * libm::cosf(y),
                libm::coshf(x) * libm::sinf(y),
            );
        }
        if ix < 0x42b1_7218 {
            let h = libm::expf(x.abs()) * 0.5;
            return c32(h.copysign(x) * libm::cosf(y), h * libm::sinf(y));
        } else if ix < 0x4340_b1e7 {
            let w = ldexp_cexpf(x.abs(), y, -1);
            return c32(w.re * 1.0_f32.copysign(x), w.im);
        }
        let h = HUGE * x;
        return c32(h * libm::cosf(y), h * h * libm::sinf(y));
    }

    if ix == 0 {
        return c32(x, y - y);
    }
    if iy == 0 {
        return c32(x + x, y);
    }
    if ix < 0x7f80_0000 {
        return c32(y - y, y - y);
    }
    if ix == 0x7f80_0000 {
        if iy >= 0x7f80_0000 {
            return c32(f32::INFINITY, y - y);
        }
        return c32(x * libm::cosf(y), f32::INFINITY * libm::sinf(y));
    }
    c32((x + x) * (y - y), (x * x) * (y - y))
}

/// [`csin`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn csinf(z: Complex32) -> Complex32 {
    let w = csinhf(c32(z.im, z.re));
    c32(w.im, w.re)
}

/// [`ctanh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctanhf(z: Complex32) -> Complex32 {
    let (mut x, y) = (z.re, z.im);
    let hx = x.to_bits();
    let ix = hx & 0x7fff_ffff;

    if ix >= 0x7f80_0000 {
        if ix & 0x7f_ffff != 0 {
            return c32(nan_mixf(x, y), if y == 0.0 { y } else { nan_mixf(x, y) });
        }
        x = f32::from_bits(hx.wrapping_sub(0x4000_0000));
        let s = if y.is_infinite() {
            y
        } else {
            libm::sinf(y) * libm::cosf(y)
        };
        return c32(x, 0.0_f32.copysign(s));
    }

    if !y.is_finite() {
        return c32(if ix != 0 { y - y } else { x }, y - y);
    }

    if ix >= 0x4130_0000 {
        // |x| >= 11
        let exp_mx = libm::expf(-x.abs());
        return c32(
            1.0_f32.copysign(x),
            4.0 * libm::sinf(y) * libm::cosf(y) * exp_mx * exp_mx,
        );
    }

    let t = libm::tanf(y);
    // `1.0 + t * t` is a double sum in the C, rounded once to float; the
    // exact double sum of two floats rounds to the same float as a float
    // addition does, so float arithmetic gives the identical value.
    let beta = 1.0 + t * t;
    let s = libm::sinhf(x);
    let rho = libm::sqrtf(1.0 + s * s);
    let denom = 1.0 + beta * s * s;
    c32((beta * rho * s) / denom, t / denom)
}

/// [`ctan`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ctanf(z: Complex32) -> Complex32 {
    let w = ctanhf(c32(z.im, z.re));
    c32(w.im, w.re)
}

// ===========================================================================
// casinh, casin, cacos, cacosh, catanh, catan -- FreeBSD catrig.c /
// catrigf.c
//
// Copyright (c) 2012 Stephen Montgomery-Smith <stephen@FreeBSD.ORG>
// All rights reserved. (BSD-2-Clause; the full text is at the end of this
// file.)
//
// Throughout, z = x + I*y.
//
// casinh(z) = sign(x)*log(A+sqrt(A*A-1)) + I*asin(B)
// where A = (|z+I| + |z-I|) / 2 and B = (|z+I| - |z-I|) / 2 = y/A.
//
// These become unstable (a) for Re(casinh(z)) when z is close to the segment
// [-I, I], and (b) for Im(casinh(z)) when z is close to [I, I*inf) or
// (-I*inf, -I]. Both are overcome through
//   f(a, b) = (hypot(a, b) - b) / 2 = a*a / (hypot(a, b) + b) / 2:
// if A < A_crossover, log(A + sqrt(A*A-1)) = log1p((A-1) + sqrt((A-1)*(A+1)))
// with A-1 = f(x, 1+y) + f(x, 1-y); if B > B_crossover,
// asin(B) = atan2(y, sqrt((A+y)*(A-y))) with A-y = f(x, y+1) + f(x, y-1)
// (x, y >= 0 without loss of generality). Where the intermediate
// computations would overflow or underflow, the paper's exception handling is
// replaced by detecting the risk first.
// ===========================================================================

/// The double-precision constants of `catrig.c`.
mod k64 {
    use super::p2;
    pub(super) const A_CROSSOVER: f64 = 10.0; // Hull et al suggest 1.5, but 10 works better
    pub(super) const B_CROSSOVER: f64 = 0.6417; // suggested by Hull et al
    pub(super) const FOUR_SQRT_MIN: f64 = p2(-509); // >= 4 * sqrt(DBL_MIN)
    pub(super) const QUARTER_SQRT_MAX: f64 = p2(509); // <= sqrt(DBL_MAX) / 4
    pub(super) const M_E: f64 = core::f64::consts::E; // 0x15bf0a8b145769.0p-51, FreeBSD's m_e
    pub(super) const M_LN2: f64 = core::f64::consts::LN_2; // 0x162e42fefa39ef.0p-53
    pub(super) const PIO2_HI: f64 = core::f64::consts::FRAC_PI_2; // 0x1921fb54442d18.0p-52
    pub(super) const PIO2_LO: f64 = 6.123_233_995_736_766e-17; // 0x11a62633145c07.0p-106
    pub(super) const RECIP_EPSILON: f64 = 1.0 / f64::EPSILON;
    // FreeBSD's source gives these two in decimal and in a hex comment that
    // differ by an ulp; its compiled code has the decimal's value, so these do.
    // They are thresholds, where an ulp moves nothing.
    pub(super) const SQRT_3_EPSILON: f64 = 2.580_956_827_951_785e-8; // ~0x1bb67ae8584caa.0p-78
    pub(super) const SQRT_6_EPSILON: f64 = 3.650_024_149_988_857_4e-8; // ~0x13988e1409212e.0p-77
    pub(super) const SQRT_MIN: f64 = p2(-511); // >= sqrt(DBL_MIN)
}

/// `pio2_hi + pio2_lo`, which FreeBSD keeps `volatile` so that the sum is
/// computed -- raising inexact -- rather than folded.
fn pio2() -> f64 {
    k64::PIO2_HI + core::hint::black_box(k64::PIO2_LO)
}

/// `f(a, b, hypot(a, b)) = (hypot(a, b) - b) / 2`, without cancellation.
fn catrig_f(a: f64, b: f64, hypot_a_b: f64) -> f64 {
    if b < 0.0 {
        return (hypot_a_b - b) / 2.0;
    }
    if b == 0.0 {
        return a / 2.0;
    }
    a * a / (hypot_a_b + b) / 2.0
}

/// What [`do_hard_work`] found: `rx = Re(casinh(z)) = -Im(cacos(y + I*x))`,
/// and either a usable `B` or, when it is not, `sqrt(A*A - y*y)` with `y`,
/// rescaled together where returning them unscaled could underflow.
struct HardWork {
    rx: f64,
    b: Option<f64>,
    sqrt_a2my2: f64,
    new_y: f64,
}

/// All the hard work. `x` and `y` are non-negative and below
/// `RECIP_EPSILON`.
fn do_hard_work(x: f64, y: f64) -> HardWork {
    use k64::*;
    const EPS: f64 = f64::EPSILON;
    let r = libm::hypot(x, y + 1.0); // |z+I|
    let s = libm::hypot(x, y - 1.0); // |z-I|

    // A = (|z+I| + |z-I|) / 2, mathematically >= 1; rounding could make it
    // less, so make certain.
    let a = ((r + s) / 2.0).max(1.0);

    let rx = if a < A_CROSSOVER {
        // Am1 = fp + fm, where fp = f(x, 1+y) and fm = f(x, 1-y);
        // rx = log1p(Am1 + sqrt(Am1*(A+1))).
        if y == 1.0 && x < EPS * EPS / 128.0 {
            // fp is of order x^2, and fm = x/2; A = 1 (inexactly).
            libm::sqrt(x)
        } else if x >= EPS * (y - 1.0).abs() {
            // Underflow will not occur because x >= DBL_EPSILON^2/128 >=
            // FOUR_SQRT_MIN.
            let am1 = catrig_f(x, 1.0 + y, r) + catrig_f(x, 1.0 - y, s);
            libm::log1p(am1 + libm::sqrt(am1 * (a + 1.0)))
        } else if y < 1.0 {
            // fp = x*x/(1+y)/4, fm = x*x/(1-y)/4, and A = 1 (inexactly).
            x / libm::sqrt((1.0 - y) * (1.0 + y))
        } else {
            // y > 1: A-1 = y-1 (inexactly).
            libm::log1p((y - 1.0) + libm::sqrt((y - 1.0) * (y + 1.0)))
        }
    } else {
        libm::log(a + libm::sqrt(a * a - 1.0))
    };

    if y < FOUR_SQRT_MIN {
        // Avoid a possible underflow caused by y/A. For casinh this would be
        // legitimate, but will be picked up by invoking atan2 later on. For
        // cacos this would not be legitimate.
        return HardWork {
            rx,
            b: None,
            sqrt_a2my2: a * (2.0 / EPS),
            new_y: y * (2.0 / EPS),
        };
    }

    // B = (|z+I| - |z-I|) / 2 = y/A
    let b = y / a;
    if b <= B_CROSSOVER {
        return HardWork {
            rx,
            b: Some(b),
            sqrt_a2my2: 0.0,
            new_y: y,
        };
    }

    // Amy = fp + fm, where fp = f(x, y+1) and fm = f(x, y-1);
    // sqrt_A2my2 = sqrt(Amy*(A+y)).
    let (sqrt_a2my2, new_y) = if y == 1.0 && x < EPS / 128.0 {
        // fp is of order x^2, and fm = x/2; A = 1 (inexactly).
        (libm::sqrt(x) * libm::sqrt((a + y) / 2.0), y)
    } else if x >= EPS * (y - 1.0).abs() {
        // Underflow will not occur because x >= DBL_EPSILON/128 >=
        // FOUR_SQRT_MIN and x >= DBL_EPSILON^2 >= FOUR_SQRT_MIN.
        let amy = catrig_f(x, y + 1.0, r) + catrig_f(x, y - 1.0, s);
        (libm::sqrt(amy * (a + y)), y)
    } else if y > 1.0 {
        // fp = x*x/(y+1)/4, fm = x*x/(y-1)/4, and A = y (inexactly). y <
        // RECIP_EPSILON, so this scaling avoids any underflow.
        (
            x * (4.0 / EPS / EPS) * y / libm::sqrt((y + 1.0) * (y - 1.0)),
            y * (4.0 / EPS / EPS),
        )
    } else {
        // y < 1: fm = 1-y >= DBL_EPSILON, fp is of order x^2, and A = 1
        // (inexactly).
        (libm::sqrt((1.0 - y) * (1.0 + y)), y)
    };
    HardWork {
        rx,
        b: None,
        sqrt_a2my2,
        new_y,
    }
}

/// `clog` for `|z|` finite and larger than about `RECIP_EPSILON`.
fn clog_for_large_values(x: f64, y: f64) -> Complex64 {
    use k64::*;
    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }
    // Avoid overflow in hypot() when x and y are both very large: divide by
    // E, which is larger than sqrt(2), and add 1 to the logarithm.
    if ax > f64::MAX / 2.0 {
        return c64(
            libm::log(libm::hypot(x / M_E, y / M_E)) + 1.0,
            libm::atan2(y, x),
        );
    }
    // Avoid overflow when x or y is large, and underflow when either is
    // small.
    if ax > QUARTER_SQRT_MAX || ay < SQRT_MIN {
        return c64(libm::log(libm::hypot(x, y)), libm::atan2(y, x));
    }
    c64(libm::log(ax * ax + ay * ay) / 2.0, libm::atan2(y, x))
}

/// The inverse hyperbolic sine, with branch cuts along the imaginary axis
/// outside `[-i, i]`.
///
/// `casinh(z) = z + O(z^3)` as `z -> 0`, and
/// `casinh(z) = sign(x)*clog(sign(x)*z) + O(1/z^2)` as `z -> infinity`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn casinh(z: Complex64) -> Complex64 {
    use k64::*;
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        // casinh(+-Inf + I*NaN) = +-Inf + I*NaN
        if x.is_infinite() {
            return c64(x, y + y);
        }
        // casinh(NaN + I*+-Inf) = opt(+-)Inf + I*NaN. The sign is open;
        // FreeBSD takes y's, glibc (`s_casinh_template.c`) copysign(Inf, x)
        // -- the NaN's own sign bit -- and so does this.
        if y.is_infinite() {
            return c64(f64::INFINITY.copysign(x), x + x);
        }
        // casinh(NaN + I*0) = NaN + I*0
        if y == 0.0 {
            return c64(x + x, y);
        }
        // Every other case involving NaN is NaN + I*NaN; raising invalid is
        // optional when one argument is not NaN, and not done.
        return c64(nan_mix(x, y), nan_mix(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        // clog...() raises inexact unless x or y is infinite.
        let w = if x.is_sign_negative() {
            clog_for_large_values(-x, -y)
        } else {
            clog_for_large_values(x, y)
        };
        return c64((w.re + M_LN2).copysign(x), w.im.copysign(y));
    }

    // Avoid spuriously raising inexact for z = 0.
    if x == 0.0 && y == 0.0 {
        return z;
    }

    // All remaining cases are inexact.
    raise_inexact();

    if ax < SQRT_6_EPSILON / 4.0 && ay < SQRT_6_EPSILON / 4.0 {
        return z;
    }

    let w = do_hard_work(ax, ay);
    let ry = match w.b {
        Some(b) => libm::asin(b),
        None => libm::atan2(w.new_y, w.sqrt_a2my2),
    };
    c64(w.rx.copysign(x), ry.copysign(y))
}

/// The inverse sine: `casinh` with the parts exchanged on the way in and
/// out (`reverse(x + I*y) = y + I*x = I*conj(z)`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn casin(z: Complex64) -> Complex64 {
    let w = casinh(c64(z.im, z.re));
    c64(w.im, w.re)
}

/// The inverse cosine, `pi/2 - casin(z)` computed so that it stays accurate
/// near `z = 1`, with branch cuts along the real axis outside `[-1, 1]`.
///
/// `cacos(z) = PI/2 - z + O(z^3)` as `z -> 0`, and
/// `cacos(z) = -sign(y)*I*clog(z) + O(1/z^2)` as `z -> infinity`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cacos(z: Complex64) -> Complex64 {
    use k64::*;
    let (x, y) = (z.re, z.im);
    let (sx, sy) = (x.is_sign_negative(), y.is_sign_negative());
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        // cacos(+-Inf + I*NaN) = NaN + I*opt(-)Inf
        if x.is_infinite() {
            return c64(y + y, f64::NEG_INFINITY);
        }
        // cacos(NaN + I*+-Inf) = NaN + I*-+Inf
        if y.is_infinite() {
            return c64(x + x, -y);
        }
        // cacos(0 + I*NaN) = PI/2 + I*NaN with inexact
        if x == 0.0 {
            return c64(pio2(), y + y);
        }
        return c64(nan_mix(x, y), nan_mix(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        // clog...() raises inexact unless x or y is infinite.
        let w = clog_for_large_values(x, y);
        let rx = w.im.abs();
        let ry = w.re + M_LN2;
        return c64(rx, if sy { ry } else { -ry });
    }

    // Avoid spuriously raising inexact for z = 1.
    if x == 1.0 && y == 0.0 {
        return c64(0.0, -y);
    }

    // All remaining cases are inexact.
    raise_inexact();

    if ax < SQRT_6_EPSILON / 4.0 && ay < SQRT_6_EPSILON / 4.0 {
        return c64(PIO2_HI - (x - core::hint::black_box(PIO2_LO)), -y);
    }

    let w = do_hard_work(ay, ax);
    let (ry, new_x, sqrt_a2mx2) = (w.rx, w.new_y, w.sqrt_a2my2);
    let rx = match w.b {
        Some(b) => libm::acos(if sx { -b } else { b }),
        None => libm::atan2(sqrt_a2mx2, if sx { -new_x } else { new_x }),
    };
    c64(rx, if sy { ry } else { -ry })
}

/// The inverse hyperbolic cosine: `I*cacos(z)` or `-I*cacos(z)`, whichever
/// has a non-negative real part.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cacosh(z: Complex64) -> Complex64 {
    let w = cacos(z);
    let (rx, ry) = (w.re, w.im);
    // cacosh(NaN + I*NaN) = NaN + I*NaN
    if rx.is_nan() && ry.is_nan() {
        return c64(ry, rx);
    }
    // cacosh(NaN + I*+-Inf) = +Inf + I*NaN; cacosh(+-Inf + I*NaN) = +Inf +
    // I*NaN
    if rx.is_nan() {
        return c64(ry.abs(), rx);
    }
    // cacosh(+-0 + I*NaN) = NaN + I*pi/2: cacos gave pi/2 + I*NaN, and
    // glibc (`s_cacosh_template.c`) keeps the pi/2, where FreeBSD answers
    // NaN + I*NaN. Annex G leaves it open; this answers as glibc does.
    if ry.is_nan() {
        return c64(ry, rx);
    }
    c64(ry.abs(), rx.copysign(z.im))
}

/// `x*x + y*y`, or just `x*x` when `y*y` would underflow. `x` and `y` are
/// finite, `y` is non-negative, `|x| >= DBL_EPSILON`, and neither square
/// overflows.
fn sum_squares(x: f64, y: f64) -> f64 {
    if y < k64::SQRT_MIN {
        return x * x;
    }
    x * x + y * y
}

/// `Re(1/(x + I*y)) = x/(x*x + y*y)`, for `x` and `y` not NaN and one of
/// them above `RECIP_EPSILON`, without the unwarranted underflow `creal(1/z)`
/// could give through its imaginary part (after C99 n1124 G.5.1, example 2).
/// Only called where inexact has already been raised.
fn real_part_reciprocal(x: f64, y: f64) -> f64 {
    const BIAS: i32 = 1024 - 1; // DBL_MAX_EXP - 1
    const CUTOFF: i32 = 53 / 2 + 1; // DBL_MANT_DIG / 2 + 1: one guard digit
    let ix = (hi(x) & 0x7ff0_0000) as i32;
    let iy = (hi(y) & 0x7ff0_0000) as i32;
    if ix.wrapping_sub(iy) >= CUTOFF << 20 || x.is_infinite() {
        return 1.0 / x; // +-Inf -> +-0 is special
    }
    if iy.wrapping_sub(ix) >= CUTOFF << 20 {
        return x / y / y; // should avoid double div, but hard
    }
    if ix <= (BIAS + 1024 / 2 - CUTOFF) << 20 {
        return x / (x * x + y * y);
    }
    // 2**(1-ilogb(x))
    let scale = with_hi(1.0, (0x7ff0_0000_i32.wrapping_sub(ix)) as u32);
    let (x, y) = (x * scale, y * scale);
    x / (x * x + y * y) * scale
}

/// The inverse hyperbolic tangent, with branch cuts along the real axis
/// outside `[-1, 1]`:
/// `log((1+z)/(1-z)) / 2 = log1p(4*x / |z-1|^2) / 4 + I * atan2(2*y,
/// (1-x)*(1+x)-y*y) / 2`.
///
/// `catanh(z) = z + O(z^3)` as `z -> 0`, and
/// `catanh(z) = 1/z + sign(y)*I*PI/2 + O(1/z^3)` as `z -> infinity`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catanh(z: Complex64) -> Complex64 {
    use k64::*;
    const EPS: f64 = f64::EPSILON;
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());

    // This helps handle many cases. `atanh` with errno: at x = +-1 glibc's
    // catanh takes log(0) on the way and reports ERANGE, as `atanh` does.
    if y == 0.0 && ax <= 1.0 {
        return c64(crate::math::atanh(x), y);
    }

    // To ensure the same accuracy as atan(), and to filter out z = 0.
    if x == 0.0 {
        return c64(x, libm::atan(y));
    }

    if x.is_nan() || y.is_nan() {
        // catanh(+-Inf + I*NaN) = +-0 + I*NaN
        if x.is_infinite() {
            return c64(0.0_f64.copysign(x), y + y);
        }
        // catanh(NaN + I*+-Inf) = sign(NaN)0 + I*+-PI/2
        if y.is_infinite() {
            return c64(0.0_f64.copysign(x), pio2().copysign(y));
        }
        return c64(nan_mix(x, y), nan_mix(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        return c64(real_part_reciprocal(x, y), pio2().copysign(y));
    }

    if ax < SQRT_3_EPSILON / 2.0 && ay < SQRT_3_EPSILON / 2.0 {
        // z = 0 was filtered out above. All other cases must raise inexact,
        // but this is the only one that needs to do it explicitly.
        raise_inexact();
        return z;
    }

    let rx = if ax == 1.0 && ay < EPS {
        (M_LN2 - libm::log(ay)) / 2.0
    } else {
        libm::log1p(4.0 * ax / sum_squares(ax - 1.0, ay)) / 4.0
    };

    let ry = if ax == 1.0 {
        libm::atan2(2.0, -ay) / 2.0
    } else if ay < EPS {
        libm::atan2(2.0 * ay, (1.0 - ax) * (1.0 + ax)) / 2.0
    } else {
        libm::atan2(2.0 * ay, (1.0 - ax) * (1.0 + ax) - ay * ay) / 2.0
    };

    c64(rx.copysign(x), ry.copysign(y))
}

/// The inverse tangent: `catanh` with the parts exchanged on the way in and
/// out.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catan(z: Complex64) -> Complex64 {
    let w = catanh(c64(z.im, z.re));
    c64(w.im, w.re)
}

/// The single-precision constants of `catrigf.c`.
mod k32 {
    use super::p2f;
    pub(super) const A_CROSSOVER: f32 = 10.0;
    pub(super) const B_CROSSOVER: f32 = 0.6417;
    pub(super) const FOUR_SQRT_MIN: f32 = p2f(-61);
    pub(super) const QUARTER_SQRT_MAX: f32 = p2f(61);
    pub(super) const M_E: f32 = core::f32::consts::E; // 0xadf854.0p-22
    pub(super) const M_LN2: f32 = core::f32::consts::LN_2; // 0xb17218.0p-24
    pub(super) const PIO2_HI: f32 = 1.570_796_3; // 0xc90fda.0p-23: pi/2 truncated, not rounded
    pub(super) const PIO2_LO: f32 = 7.549_79e-8; // 0xa22169.0p-47
    pub(super) const RECIP_EPSILON: f32 = 1.0 / f32::EPSILON;
    pub(super) const SQRT_3_EPSILON: f32 = 5.980_2e-4; // 0x9cc471.0p-34
    pub(super) const SQRT_6_EPSILON: f32 = 8.457_279e-4; // 0xddb3d7.0p-34
    pub(super) const SQRT_MIN: f32 = p2f(-63);
}

fn pio2f() -> f32 {
    k32::PIO2_HI + core::hint::black_box(k32::PIO2_LO)
}

fn catrig_ff(a: f32, b: f32, hypot_a_b: f32) -> f32 {
    if b < 0.0 {
        return (hypot_a_b - b) / 2.0;
    }
    if b == 0.0 {
        return a / 2.0;
    }
    a * a / (hypot_a_b + b) / 2.0
}

struct HardWorkF {
    rx: f32,
    b: Option<f32>,
    sqrt_a2my2: f32,
    new_y: f32,
}

fn do_hard_workf(x: f32, y: f32) -> HardWorkF {
    use k32::*;
    const EPS: f32 = f32::EPSILON;
    let r = libm::hypotf(x, y + 1.0);
    let s = libm::hypotf(x, y - 1.0);
    let a = ((r + s) / 2.0).max(1.0);

    let rx = if a < A_CROSSOVER {
        if y == 1.0 && x < EPS * EPS / 128.0 {
            libm::sqrtf(x)
        } else if x >= EPS * (y - 1.0).abs() {
            let am1 = catrig_ff(x, 1.0 + y, r) + catrig_ff(x, 1.0 - y, s);
            libm::log1pf(am1 + libm::sqrtf(am1 * (a + 1.0)))
        } else if y < 1.0 {
            x / libm::sqrtf((1.0 - y) * (1.0 + y))
        } else {
            libm::log1pf((y - 1.0) + libm::sqrtf((y - 1.0) * (y + 1.0)))
        }
    } else {
        libm::logf(a + libm::sqrtf(a * a - 1.0))
    };

    if y < FOUR_SQRT_MIN {
        return HardWorkF {
            rx,
            b: None,
            sqrt_a2my2: a * (2.0 / EPS),
            new_y: y * (2.0 / EPS),
        };
    }

    let b = y / a;
    if b <= B_CROSSOVER {
        return HardWorkF {
            rx,
            b: Some(b),
            sqrt_a2my2: 0.0,
            new_y: y,
        };
    }

    let (sqrt_a2my2, new_y) = if y == 1.0 && x < EPS / 128.0 {
        (libm::sqrtf(x) * libm::sqrtf((a + y) / 2.0), y)
    } else if x >= EPS * (y - 1.0).abs() {
        let amy = catrig_ff(x, y + 1.0, r) + catrig_ff(x, y - 1.0, s);
        (libm::sqrtf(amy * (a + y)), y)
    } else if y > 1.0 {
        (
            x * (4.0 / EPS / EPS) * y / libm::sqrtf((y + 1.0) * (y - 1.0)),
            y * (4.0 / EPS / EPS),
        )
    } else {
        (libm::sqrtf((1.0 - y) * (1.0 + y)), y)
    };
    HardWorkF {
        rx,
        b: None,
        sqrt_a2my2,
        new_y,
    }
}

fn clog_for_large_valuesf(x: f32, y: f32) -> Complex32 {
    use k32::*;
    let (mut ax, mut ay) = (x.abs(), y.abs());
    if ax < ay {
        core::mem::swap(&mut ax, &mut ay);
    }
    if ax > f32::MAX / 2.0 {
        return c32(
            libm::logf(libm::hypotf(x / M_E, y / M_E)) + 1.0,
            libm::atan2f(y, x),
        );
    }
    if ax > QUARTER_SQRT_MAX || ay < SQRT_MIN {
        return c32(libm::logf(libm::hypotf(x, y)), libm::atan2f(y, x));
    }
    c32(libm::logf(ax * ax + ay * ay) / 2.0, libm::atan2f(y, x))
}

/// [`casinh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn casinhf(z: Complex32) -> Complex32 {
    use k32::*;
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return c32(x, y + y);
        }
        if y.is_infinite() {
            return c32(f32::INFINITY.copysign(x), x + x);
        }
        if y == 0.0 {
            return c32(x + x, y);
        }
        return c32(nan_mixf(x, y), nan_mixf(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        let w = if x.is_sign_negative() {
            clog_for_large_valuesf(-x, -y)
        } else {
            clog_for_large_valuesf(x, y)
        };
        return c32((w.re + M_LN2).copysign(x), w.im.copysign(y));
    }

    if x == 0.0 && y == 0.0 {
        return z;
    }

    raise_inexact();

    if ax < SQRT_6_EPSILON / 4.0 && ay < SQRT_6_EPSILON / 4.0 {
        return z;
    }

    let w = do_hard_workf(ax, ay);
    let ry = match w.b {
        Some(b) => libm::asinf(b),
        None => libm::atan2f(w.new_y, w.sqrt_a2my2),
    };
    c32(w.rx.copysign(x), ry.copysign(y))
}

/// [`casin`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn casinf(z: Complex32) -> Complex32 {
    let w = casinhf(c32(z.im, z.re));
    c32(w.im, w.re)
}

/// [`cacos`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cacosf(z: Complex32) -> Complex32 {
    use k32::*;
    let (x, y) = (z.re, z.im);
    let (sx, sy) = (x.is_sign_negative(), y.is_sign_negative());
    let (ax, ay) = (x.abs(), y.abs());

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return c32(y + y, f32::NEG_INFINITY);
        }
        if y.is_infinite() {
            return c32(x + x, -y);
        }
        if x == 0.0 {
            return c32(pio2f(), y + y);
        }
        return c32(nan_mixf(x, y), nan_mixf(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        let w = clog_for_large_valuesf(x, y);
        let rx = w.im.abs();
        let ry = w.re + M_LN2;
        return c32(rx, if sy { ry } else { -ry });
    }

    if x == 1.0 && y == 0.0 {
        return c32(0.0, -y);
    }

    raise_inexact();

    if ax < SQRT_6_EPSILON / 4.0 && ay < SQRT_6_EPSILON / 4.0 {
        return c32(PIO2_HI - (x - core::hint::black_box(PIO2_LO)), -y);
    }

    let w = do_hard_workf(ay, ax);
    let (ry, new_x, sqrt_a2mx2) = (w.rx, w.new_y, w.sqrt_a2my2);
    let rx = match w.b {
        Some(b) => libm::acosf(if sx { -b } else { b }),
        None => libm::atan2f(sqrt_a2mx2, if sx { -new_x } else { new_x }),
    };
    c32(rx, if sy { ry } else { -ry })
}

/// [`cacosh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cacoshf(z: Complex32) -> Complex32 {
    let w = cacosf(z);
    let (rx, ry) = (w.re, w.im);
    if rx.is_nan() && ry.is_nan() {
        return c32(ry, rx);
    }
    if rx.is_nan() {
        return c32(ry.abs(), rx);
    }
    if ry.is_nan() {
        return c32(ry, rx);
    }
    c32(ry.abs(), rx.copysign(z.im))
}

fn sum_squaresf(x: f32, y: f32) -> f32 {
    if y < k32::SQRT_MIN {
        return x * x;
    }
    x * x + y * y
}

fn real_part_reciprocalf(x: f32, y: f32) -> f32 {
    const BIAS: i32 = 128 - 1; // FLT_MAX_EXP - 1
    const CUTOFF: i32 = 24 / 2 + 1; // FLT_MANT_DIG / 2 + 1
    let ix = (x.to_bits() & 0x7f80_0000) as i32;
    let iy = (y.to_bits() & 0x7f80_0000) as i32;
    if ix.wrapping_sub(iy) >= CUTOFF << 23 || x.is_infinite() {
        return 1.0 / x;
    }
    if iy.wrapping_sub(ix) >= CUTOFF << 23 {
        return x / y / y;
    }
    if ix <= (BIAS + 128 / 2 - CUTOFF) << 23 {
        return x / (x * x + y * y);
    }
    let scale = f32::from_bits((0x7f80_0000_i32.wrapping_sub(ix)) as u32);
    let (x, y) = (x * scale, y * scale);
    x / (x * x + y * y) * scale
}

/// [`catanh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catanhf(z: Complex32) -> Complex32 {
    use k32::*;
    const EPS: f32 = f32::EPSILON;
    let (x, y) = (z.re, z.im);
    let (ax, ay) = (x.abs(), y.abs());

    if y == 0.0 && ax <= 1.0 {
        return c32(crate::math::atanhf(x), y);
    }
    if x == 0.0 {
        return c32(x, libm::atanf(y));
    }

    if x.is_nan() || y.is_nan() {
        if x.is_infinite() {
            return c32(0.0_f32.copysign(x), y + y);
        }
        if y.is_infinite() {
            return c32(0.0_f32.copysign(x), pio2f().copysign(y));
        }
        return c32(nan_mixf(x, y), nan_mixf(x, y));
    }

    if ax > RECIP_EPSILON || ay > RECIP_EPSILON {
        return c32(real_part_reciprocalf(x, y), pio2f().copysign(y));
    }

    if ax < SQRT_3_EPSILON / 2.0 && ay < SQRT_3_EPSILON / 2.0 {
        raise_inexact();
        return z;
    }

    let rx = if ax == 1.0 && ay < EPS {
        (M_LN2 - libm::logf(ay)) / 2.0
    } else {
        libm::log1pf(4.0 * ax / sum_squaresf(ax - 1.0, ay)) / 4.0
    };

    let ry = if ax == 1.0 {
        libm::atan2f(2.0, -ay) / 2.0
    } else if ay < EPS {
        libm::atan2f(2.0 * ay, (1.0 - ax) * (1.0 + ax)) / 2.0
    } else {
        libm::atan2f(2.0 * ay, (1.0 - ax) * (1.0 + ax) - ay * ay) / 2.0
    };

    c32(rx.copysign(x), ry.copysign(y))
}

/// [`catan`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn catanf(z: Complex32) -> Complex32 {
    let w = catanhf(c32(z.im, z.re));
    c32(w.im, w.re)
}

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

    // -----------------------------------------------------------------------
    // glibc 2.39, call by call (dlm/oracle/complex_harness.py)
    // -----------------------------------------------------------------------

    /// The glibc oracle's table: one call a line, `<function> <re> <im>
    /// [<re2> <im2>] = <outputs> <errno>`, floats as their bits in hex.
    const ORACLE: &str = include_str!("complex_oracle.txt");

    /// How far from glibc's answer each part of ours may be, in units in the
    /// last place.
    ///
    /// 0 where the answer is exact by definition. Elsewhere glibc's
    /// algorithms and FreeBSD's differ, and so do the real functions under
    /// them (glibc's and musl's): the bound is the sum of glibc's documented
    /// worst case for the function (`libm-test-ulps`, x86_64), FreeBSD's
    /// ("accurate up to 4 ULP" for catrig.c; 1 to 2 elsewhere) and one for
    /// the real functions' own difference, rounded up. A difference past it
    /// is a bug in one of the three.
    fn ulps_allowed(name: &str) -> u64 {
        match name.strip_suffix('f').unwrap_or(name) {
            "creal" | "cimag" | "conj" | "cproj" => 0,
            "cabs" | "carg" => 2,
            "cexp" | "csin" | "ccos" | "csinh" | "ccosh" | "csqrt" => 4,
            "clog" | "ctan" | "ctanh" => 6,
            "casin" | "cacos" | "casinh" | "cacosh" | "catan" | "catanh" => 8,
            other => panic!("no tolerance for {other}"),
        }
    }

    /// `cpow` is `cexp(y * clog(x))` in both libraries, so every ulp `clog`
    /// differs by is multiplied by `|y|` and then exponentiated: the parts
    /// are compared together, as a relative error in the whole result,
    /// bounded well above what that amplification can produce for the
    /// table's `|x|, |y| <= 4` (about `10 * |y|` ulps).
    const CPOW_RELATIVE: f64 = 1e-13;
    const CPOWF_RELATIVE: f64 = 1e-5;

    /// Bits laid out on one ordered line, so the step across zero is one step
    /// like any other.
    fn ulp_distance(a: u64, b: u64, sign_bit: u32) -> u64 {
        fn line(b: u64, sign_bit: u32) -> i128 {
            let mag = i128::from(b & ((1u64 << sign_bit) - 1));
            if b >> sign_bit & 1 == 1 { -mag } else { mag }
        }
        u64::try_from((line(a, sign_bit) - line(b, sign_bit)).unsigned_abs()).unwrap_or(u64::MAX)
    }

    /// One part, ours against glibc's: a NaN matches any NaN (a computed
    /// NaN's payload is not specified); an infinity or a zero must match
    /// exactly, sign included -- those are Annex G's special values;
    /// anything else within `tol` ulps.
    fn same_part(
        ours: f64,
        ours_bits: u64,
        glibc: f64,
        glibc_bits: u64,
        sign_bit: u32,
        tol: u64,
    ) -> Result<(), String> {
        if ours.is_nan() || glibc.is_nan() {
            return if ours.is_nan() && glibc.is_nan() {
                Ok(())
            } else {
                Err(format!("{ours:e}, glibc {glibc:e}"))
            };
        }
        if ours.is_infinite() || glibc.is_infinite() || (ours == 0.0 && glibc == 0.0) {
            return if ours_bits == glibc_bits {
                Ok(())
            } else {
                Err(format!(
                    "{ours:e} ({ours_bits:#x}), glibc {glibc:e} ({glibc_bits:#x})"
                ))
            };
        }
        // A zero against a nonzero is an accuracy question, not a special
        // value: glibc's csqrt rounds twice and flushes a result FreeBSD's
        // rounds once to the smallest subnormal. Same sign, and within the
        // tolerance, like any other pair.
        if (ours == 0.0 || glibc == 0.0) && ours.is_sign_negative() != glibc.is_sign_negative() {
            return Err(format!(
                "{ours:e} ({ours_bits:#x}), glibc {glibc:e} ({glibc_bits:#x}): the sign"
            ));
        }
        let d = ulp_distance(ours_bits, glibc_bits, sign_bit);
        if d > tol {
            return Err(format!(
                "{ours:e} ({ours_bits:#x}), glibc {glibc:e} ({glibc_bits:#x}): {d} ulp"
            ));
        }
        Ok(())
    }

    fn d(h: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(h, 16).unwrap())
    }

    fn f(h: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(h, 16).unwrap())
    }

    fn part_d(ours: f64, glibc: f64, tol: u64) -> Result<(), String> {
        same_part(ours, ours.to_bits(), glibc, glibc.to_bits(), 63, tol)
    }

    fn part_f(ours: f32, glibc: f32, tol: u64) -> Result<(), String> {
        same_part(
            f64::from(ours),
            u64::from(ours.to_bits()),
            f64::from(glibc),
            u64::from(glibc.to_bits()),
            31,
            tol,
        )
    }

    /// `cpow`'s comparison: the special values as for the others; for finite
    /// nonzero results, the relative error of the whole.
    fn cpow_close(ours: (f64, f64), glibc: (f64, f64), rel: f64) -> Result<(), String> {
        let special = |x: f64| !x.is_finite() || x == 0.0;
        if special(glibc.0) || special(glibc.1) || special(ours.0) || special(ours.1) {
            // Both parts as special values; a finite nonzero part beside a
            // special one gets the relative test on its own.
            for (o, g) in [(ours.0, glibc.0), (ours.1, glibc.1)] {
                if special(o) || special(g) {
                    part_d(o, g, 0)?;
                } else if ((o - g) / g).abs() > rel {
                    return Err(format!("{o:e}, glibc {g:e}"));
                }
            }
            return Ok(());
        }
        let err = libm::hypot(ours.0 - glibc.0, ours.1 - glibc.1) / libm::hypot(glibc.0, glibc.1);
        if err > rel {
            return Err(format!(
                "({:e}, {:e}), glibc ({:e}, {:e}): relative {err:e}",
                ours.0, ours.1, glibc.0, glibc.1
            ));
        }
        Ok(())
    }

    /// Run one oracle line through ours. `Err` names what differed.
    fn check_line(line: &str) -> Result<(), String> {
        let (lhs, rhs) = line.split_once(" = ").ok_or("no ` = `")?;
        let mut l = lhs.split(' ');
        let name = l.next().ok_or("no name")?;
        let args: Vec<&str> = l.collect();
        let outs: Vec<&str> = rhs.split(' ').collect();
        let (vals, errno_s) = outs.split_at(outs.len() - 1);
        let errno_glibc: i32 = errno_s[0].parse().map_err(|_| "bad errno")?;
        crate::errno::set_errno(0);
        let single = name.ends_with('f');
        let tol = if name.starts_with("cpow") {
            0
        } else {
            ulps_allowed(name)
        };
        let res: Result<(), String> = if single {
            let z = c32(f(args[0]), f(args[1]));
            match name {
                "cabsf" | "cargf" | "crealf" | "cimagf" => {
                    let r = match name {
                        "cabsf" => cabsf(z),
                        "cargf" => cargf(z),
                        "crealf" => crealf(z),
                        _ => cimagf(z),
                    };
                    part_f(r, f(vals[0]), tol)
                }
                "cpowf" => {
                    let y = c32(f(args[2]), f(args[3]));
                    let r = cpowf(z, y);
                    cpow_close(
                        (f64::from(r.re), f64::from(r.im)),
                        (f64::from(f(vals[0])), f64::from(f(vals[1]))),
                        CPOWF_RELATIVE,
                    )
                }
                _ => {
                    let func: extern "C" fn(Complex32) -> Complex32 = match name {
                        "csqrtf" => csqrtf,
                        "cexpf" => cexpf,
                        "clogf" => clogf,
                        "csinf" => csinf,
                        "ccosf" => ccosf,
                        "ctanf" => ctanf,
                        "csinhf" => csinhf,
                        "ccoshf" => ccoshf,
                        "ctanhf" => ctanhf,
                        "casinf" => casinf,
                        "cacosf" => cacosf,
                        "catanf" => catanf,
                        "casinhf" => casinhf,
                        "cacoshf" => cacoshf,
                        "catanhf" => catanhf,
                        "cprojf" => cprojf,
                        "conjf" => conjf,
                        other => return Err(format!("unknown function {other}")),
                    };
                    let r = func(z);
                    part_f(r.re, f(vals[0]), tol)
                        .map_err(|e| format!("real part: {e}"))
                        .and_then(|()| {
                            part_f(r.im, f(vals[1]), tol)
                                .map_err(|e| format!("imaginary part: {e}"))
                        })
                }
            }
        } else {
            let z = c64(d(args[0]), d(args[1]));
            match name {
                "cabs" | "carg" | "creal" | "cimag" => {
                    let r = match name {
                        "cabs" => cabs(z),
                        "carg" => carg(z),
                        "creal" => creal(z),
                        _ => cimag(z),
                    };
                    part_d(r, d(vals[0]), tol)
                }
                "cpow" => {
                    let y = c64(d(args[2]), d(args[3]));
                    let r = cpow(z, y);
                    cpow_close((r.re, r.im), (d(vals[0]), d(vals[1])), CPOW_RELATIVE)
                }
                _ => {
                    let func: extern "C" fn(Complex64) -> Complex64 = match name {
                        "csqrt" => csqrt,
                        "cexp" => cexp,
                        "clog" => clog,
                        "csin" => csin,
                        "ccos" => ccos,
                        "ctan" => ctan,
                        "csinh" => csinh,
                        "ccosh" => ccosh,
                        "ctanh" => ctanh,
                        "casin" => casin,
                        "cacos" => cacos,
                        "catan" => catan,
                        "casinh" => casinh,
                        "cacosh" => cacosh,
                        "catanh" => catanh,
                        "cproj" => cproj,
                        "conj" => conj,
                        other => return Err(format!("unknown function {other}")),
                    };
                    let r = func(z);
                    part_d(r.re, d(vals[0]), tol)
                        .map_err(|e| format!("real part: {e}"))
                        .and_then(|()| {
                            part_d(r.im, d(vals[1]), tol)
                                .map_err(|e| format!("imaginary part: {e}"))
                        })
                }
            }
        };
        res?;
        let errno_ours = crate::errno::get_errno();
        if errno_ours != errno_glibc {
            return Err(format!("errno {errno_ours}, glibc {errno_glibc}"));
        }
        Ok(())
    }

    /// Every call in the table answers as glibc's does. All mismatches are
    /// collected and printed before failing, so one run shows the whole
    /// picture.
    #[test]
    fn every_call_answers_as_glibc_does() {
        let mut bad = Vec::new();
        let mut n = 0usize;
        for line in ORACLE.lines().filter(|l| !l.is_empty()) {
            n += 1;
            if let Err(e) = check_line(line) {
                bad.push(format!("{line}\n    {e}"));
            }
        }
        assert!(n > 40_000, "the table is {n} lines: truncated?");
        assert!(
            bad.is_empty(),
            "{} of {n} calls differ from glibc:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }

    /// The table covers every function this module exports, in both
    /// precisions -- so a function cannot be added without its oracle rows.
    #[test]
    fn the_table_covers_every_function() {
        const ALL: [&str; 22] = [
            "cabs", "carg", "creal", "cimag", "conj", "cproj", "csqrt", "cexp", "clog", "cpow",
            "csin", "ccos", "ctan", "csinh", "ccosh", "ctanh", "casin", "cacos", "catan", "casinh",
            "cacosh", "catanh",
        ];
        for base in ALL {
            for name in [base.to_owned(), format!("{base}f")] {
                let prefix = format!("{name} ");
                assert!(
                    ORACLE.lines().any(|l| l.starts_with(&prefix)),
                    "no rows for {name}"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The ABI, and Annex G's special values by name
    // -----------------------------------------------------------------------

    /// The types' layouts are what the SysV ABI gives `_Complex`: two parts,
    /// real first, no padding.
    #[test]
    fn the_complex_types_are_laid_out_as_c_lays_out_complex() {
        assert_eq!(
            (
                core::mem::size_of::<Complex64>(),
                core::mem::align_of::<Complex64>()
            ),
            (16, 8)
        );
        assert_eq!(
            (
                core::mem::size_of::<Complex32>(),
                core::mem::align_of::<Complex32>()
            ),
            (8, 4)
        );
        assert_eq!(core::mem::offset_of!(Complex64, im), 8);
        assert_eq!(core::mem::offset_of!(Complex32, im), 4);
    }

    fn bits(z: Complex64) -> (u64, u64) {
        (z.re.to_bits(), z.im.to_bits())
    }

    /// A few of Annex G's rules, spelled out -- the oracle checks all of
    /// them, but these say what they mean.
    #[test]
    fn annex_g_special_values() {
        let inf = f64::INFINITY;
        let pi = core::f64::consts::PI;
        // The branch cut of the square root is the negative real axis, and
        // the sign of zero picks the side.
        assert_eq!(bits(csqrt(c64(-4.0, 0.0))), bits(c64(0.0, 2.0)));
        assert_eq!(bits(csqrt(c64(-4.0, -0.0))), bits(c64(0.0, -2.0)));
        // clog of zero is -inf, raising divide-by-zero; of -1, +-i pi.
        assert_eq!(bits(clog(c64(0.0, 0.0))), bits(c64(-inf, 0.0)));
        assert_eq!(bits(clog(c64(-0.0, 0.0))), bits(c64(-inf, pi)));
        assert_eq!(clog(c64(-1.0, -0.0)).im, -pi);
        // cexp(-inf + i inf) is a zero, not NaN.
        assert_eq!(bits(cexp(c64(-inf, inf))), bits(c64(0.0, 0.0)));
        // An infinite part is infinite in cproj, whatever the other part.
        assert_eq!(bits(cproj(c64(f64::NAN, -inf))), bits(c64(inf, -0.0)));
        // conj flips the imaginary sign, a zero's included.
        assert_eq!(bits(conj(c64(1.0, 0.0))), bits(c64(1.0, -0.0)));
    }

    /// Where the textbook formulas musl uses fail and these do not: `casin`
    /// of a number whose square overflows is finite, and `clog` near `|z| =
    /// 1` keeps its digits.
    #[test]
    fn the_hard_places_stay_accurate() {
        // casin(1e300 + i0), on the upper side of the cut: pi/2 +
        // i*log(2e300). The textbook formula squares 1e300 and gets NaN.
        let w = casin(c64(1e300, 0.0));
        assert_eq!(w.re, core::f64::consts::FRAC_PI_2, "{w:?}");
        assert!((w.im - libm::log(2e300)).abs() < 1e-12, "{w:?}");

        // 0.6 + 0.8i: as doubles, 0.6 is m*2^-53 and 0.8 is n*2^-53 for the
        // integers below, so |z|^2 - 1 is (m^2 + n^2 - 2^106) * 2^-106
        // exactly -- about 4.4e-17 -- and log|z| half its log1p. The
        // textbook log(hypot(x, y)) sees hypot round to 1.0 and answers 0.
        let (m, n): (i128, i128) = (5_404_319_552_844_595, 7_205_759_403_792_794);
        let excess = (m * m + n * n - (1_i128 << 106)) as f64 * p2(-106);
        let w = clog(c64(0.6, 0.8));
        let want = libm::log1p(excess) / 2.0;
        assert!(want > 2e-17, "{want:e}");
        assert!(
            ulp_distance(w.re.to_bits(), want.to_bits(), 63) <= 2,
            "{:e}, want {want:e}",
            w.re
        );
    }
}

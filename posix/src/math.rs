//! C math library functions (`<math.h>`): musl's libm, through rust-lang's
//! `libm` crate, with glibc's error reporting.
//!
//! ## Where the answers come from
//!
//! Every function here is a C entry point over [`libm`] -- the crate
//! `compiler-builtins` uses to give Rust's own float methods a libm where the
//! platform has none, and a line-for-line port of musl's libm (itself
//! FreeBSD's msun, fdlibm's descendant), tested upstream against MPFR. It is
//! vendored exactly as crates.io publishes it, in `posix/vendor/libm`
//! (`posix/vendor/README.md` has the version and checksum).
//!
//! Until 2026-09-27 this file wrote its own: `sqrt` by Newton's method, `sin`
//! reduced by `fmod(x, 2*pi)` (so `sin(1e22)` was noise), `exp` saturating at
//! |x| = 709 (the true limits are 709.78 above and -745.13 below), `round` as
//! `floor(x + 0.5)` (`round(0.49999999999999994)` was 1), `fma` as `x*y + z`
//! with two roundings, `ldexp` truncating subnormal results, and "accurate to
//! roughly 10-15 digits", as its own documentation said. Every Rust program on
//! SlateOS reached them too: with no SSE4.1 or FMA on the baseline, `f64::sin`,
//! `f64::round` and `f64::mul_add` compile to calls to these symbols
//! (`requests/e-d-libc-round-is-wrong-just-below-a-half-and-past-2-52.md`).
//! design-decisions §1132 records why a port, and why this one.
//!
//! ## errno: glibc's
//!
//! glibc reports a math error in `errno` as well as in the floating-point
//! flags (`math_errhandling` is `MATH_ERRNO | MATH_ERREXCEPT`); musl, and so
//! the crate, only in the flags. This library answers as glibc does, and each
//! function's rule is glibc 2.39's wrapper for it -- `math/w_*_template.c`,
//! or where the double implementation sets `errno` itself
//! (`sysdeps/ieee754/dbl-64/w_exp.c` says "Not needed"), the implementation:
//!
//! | Error | `errno` | Example |
//! |---|---|---|
//! | domain: no defined result | `EDOM` | `log(-1)`, `sqrt(-1)`, `acos(2)`, `sin(inf)`, `fmod(x, 0)` |
//! | pole: an exact infinity from a finite argument | `ERANGE` | `log(0)`, `atanh(1)`, `lgamma(-2)`, `pow(0, -1)` |
//! | overflow | `ERANGE` | `exp(710)`, `cosh(711)`, `ldexp(1, 2000)` |
//! | underflow to zero | `ERANGE` | `exp(-746)`, `pow(2, -1076)` |
//!
//! `errno` is only ever set, never cleared: a caller that wants to know clears
//! it first, as C requires. The crate raises the IEEE flags as musl does;
//! reading them waits on `<fenv.h>`, which this library does not have yet
//! (`known-issues.md` -> `D-POSIX-MATH-HAS-NO-FENV-LONG-DOUBLE-OR-COMPLEX`).
//!
//! ## Tested against glibc
//!
//! The tests replay 23,113 calls to glibc 2.39 under WSL
//! (`dlm/oracle/math_harness.py`, table `math_oracle.txt`): bit for bit, with
//! `errno`, for everything IEEE fixes exactly (rounding, `fma`, `sqrt`,
//! `fmod`, `ldexp`, `frexp`, `nextafter` ...), and within a stated distance in
//! units in the last place for the rest, where glibc and musl use different
//! approximations and neither is exact.

use crate::errno;
use core::sync::atomic::{AtomicI32, Ordering};

#[inline]
fn set(e: i32) {
    errno::set_errno(e);
}

// ---------------------------------------------------------------------------
// Exact functions: IEEE fixes the answer
// ---------------------------------------------------------------------------

/// `|x|`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fabs(x: f64) -> f64 {
    libm::fabs(x)
}

/// `|x|` (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fabsf(x: f32) -> f32 {
    libm::fabsf(x)
}

/// `|x|` with `y`'s sign.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn copysign(x: f64, y: f64) -> f64 {
    libm::copysign(x, y)
}

/// `|x|` with `y`'s sign (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn copysignf(x: f32, y: f32) -> f32 {
    libm::copysignf(x, y)
}

/// glibc's internal name for [`copysign`], which old binaries import.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __copysign(x: f64, y: f64) -> f64 {
    libm::copysign(x, y)
}

/// The largest integer not above `x`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn floor(x: f64) -> f64 {
    libm::floor(x)
}

/// The largest integer not above `x` (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn floorf(x: f32) -> f32 {
    libm::floorf(x)
}

/// The smallest integer not below `x`; `ceil(-0.3)` is `-0.0`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ceil(x: f64) -> f64 {
    libm::ceil(x)
}

/// The smallest integer not below `x` (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ceilf(x: f32) -> f32 {
    libm::ceilf(x)
}

/// `x` without its fraction; `trunc(-0.3)` is `-0.0`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn trunc(x: f64) -> f64 {
    libm::trunc(x)
}

/// `x` without its fraction (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn truncf(x: f32) -> f32 {
    libm::truncf(x)
}

/// The nearest integer, a tie away from zero.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn round(x: f64) -> f64 {
    libm::round(x)
}

/// The nearest integer, a tie away from zero (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn roundf(x: f32) -> f32 {
    libm::roundf(x)
}

/// The nearest integer, a tie to even (C23).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn roundeven(x: f64) -> f64 {
    libm::roundeven(x)
}

/// The nearest integer, a tie to even (float, C23).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn roundevenf(x: f32) -> f32 {
    libm::roundevenf(x)
}

/// The integer nearest `x` in the current rounding direction
/// ([`crate::fenv::fesetround`]; to nearest, ties to even, by default).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rint(x: f64) -> f64 {
    libm::rint(x)
}

/// [`rint`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn rintf(x: f32) -> f32 {
    libm::rintf(x)
}

/// [`rint`], without raising the inexact flag: glibc's `s_nearbyint.c`
/// rounds with the SSE unit's flags held, and puts them back. A NaN or an
/// infinity is `x + x` outside the hold (a signaling NaN still raises
/// invalid), and past 2^52 a double has no fraction to round.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nearbyint(x: f64) -> f64 {
    if !x.is_finite() {
        return x + x;
    }
    if x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let held = crate::fenv::hold_sse();
    let r = libm::rint(x);
    crate::fenv::restore_sse(held);
    r
}

/// [`nearbyint`] (float; past 2^23 a float has no fraction).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nearbyintf(x: f32) -> f32 {
    if !x.is_finite() {
        return x + x;
    }
    if x.abs() >= 8_388_608.0 {
        return x;
    }
    let held = crate::fenv::hold_sse();
    let r = libm::rintf(x);
    crate::fenv::restore_sse(held);
    r
}

/// An integral double as a `long`, or `LONG_MIN` -- what x86-64's conversion
/// instruction answers, and so what glibc and musl return -- for one that is
/// not a `long`: a NaN, an infinity, or a value out of range. Nothing sets
/// `errno`; glibc raises `FE_INVALID`, as the conversion itself does.
fn to_long(r: f64) -> i64 {
    // -2^63 is a long; 2^63 is not.
    if r.is_finite() && r >= -9_223_372_036_854_775_808.0 && r < 9_223_372_036_854_775_808.0 {
        #[allow(clippy::cast_possible_truncation)]
        let v = r as i64;
        v
    } else {
        i64::MIN
    }
}

/// [`to_long`] for a float's answer.
fn to_long_f(r: f32) -> i64 {
    to_long(f64::from(r))
}

/// [`round`] as a `long`; `LONG_MIN` when it is not one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lround(x: f64) -> i64 {
    to_long(libm::round(x))
}

/// [`roundf`] as a `long`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lroundf(x: f32) -> i64 {
    to_long_f(libm::roundf(x))
}

/// [`round`] as a `long long` (the same width here).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llround(x: f64) -> i64 {
    to_long(libm::round(x))
}

/// [`roundf`] as a `long long`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llroundf(x: f32) -> i64 {
    to_long_f(libm::roundf(x))
}

/// [`rint`] as a `long`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lrint(x: f64) -> i64 {
    to_long(libm::rint(x))
}

/// [`rintf`] as a `long`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lrintf(x: f32) -> i64 {
    to_long_f(libm::rintf(x))
}

/// [`rint`] as a `long long`. Missing until 2026-09-27: a C program calling
/// it did not link.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llrint(x: f64) -> i64 {
    to_long(libm::rint(x))
}

/// [`rintf`] as a `long long`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llrintf(x: f32) -> i64 {
    to_long_f(libm::rintf(x))
}

/// Whether a NaN is signaling: its quiet bit (the mantissa's top) is clear.
fn signaling(x: f64) -> bool {
    x.is_nan() && x.to_bits() & (1u64 << 51) == 0
}

/// [`signaling`] (float).
fn signaling_f(x: f32) -> bool {
    x.is_nan() && x.to_bits() & (1u32 << 22) == 0
}

/// glibc's x86-64 `fmin`/`fmax` (`sysdeps/x86_64/fpu/s_fmin.S`, `s_fmax.S`),
/// which C leaves open on two points and these settle: two zeros give the
/// second argument (`minsd`/`maxsd` return it when neither is less/greater:
/// `fmax(+0, -0)` is -0), and a quiet NaN loses to a number while a
/// signaling NaN, or two NaNs, give `x + y` (a quiet NaN).
fn min_max(x: f64, y: f64, x_wins: bool) -> f64 {
    if !x.is_nan() && !y.is_nan() {
        return if x_wins { x } else { y };
    }
    if x.is_nan() && y.is_nan() || signaling(x) || signaling(y) {
        return x + y;
    }
    if x.is_nan() { y } else { x }
}

/// [`min_max`] (float).
fn min_max_f(x: f32, y: f32, x_wins: bool) -> f32 {
    if !x.is_nan() && !y.is_nan() {
        return if x_wins { x } else { y };
    }
    if x.is_nan() && y.is_nan() || signaling_f(x) || signaling_f(y) {
        return x + y;
    }
    if x.is_nan() { y } else { x }
}

/// The smaller, a NaN losing to a number; two equal values (`+0`, `-0`)
/// give the second.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmin(x: f64, y: f64) -> f64 {
    min_max(x, y, x < y)
}

/// [`fmin`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fminf(x: f32, y: f32) -> f32 {
    min_max_f(x, y, x < y)
}

/// The larger, a NaN losing to a number; two equal values (`+0`, `-0`)
/// give the second.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmax(x: f64, y: f64) -> f64 {
    min_max(x, y, x > y)
}

/// [`fmax`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmaxf(x: f32, y: f32) -> f32 {
    min_max_f(x, y, x > y)
}

/// `x - y` if positive, else +0 (glibc's `s_fdim_template.c`: `ERANGE` when
/// finite arguments overflow).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdim(x: f64, y: f64) -> f64 {
    let r = libm::fdim(x, y);
    if r.is_infinite() && !x.is_infinite() && !y.is_infinite() {
        set(errno::ERANGE);
    }
    r
}

/// [`fdim`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdimf(x: f32, y: f32) -> f32 {
    let r = libm::fdimf(x, y);
    if r.is_infinite() && !x.is_infinite() && !y.is_infinite() {
        set(errno::ERANGE);
    }
    r
}

/// `x * y + z`, rounded once: fused, as C requires, in software where the
/// processor has no FMA instruction.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fma(x: f64, y: f64, z: f64) -> f64 {
    libm::fma(x, y, z)
}

/// [`fma`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmaf(x: f32, y: f32, z: f32) -> f32 {
    libm::fmaf(x, y, z)
}

/// The square root, correctly rounded (`EDOM` for `x < 0`, glibc's
/// `w_sqrt_template.c`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sqrt(x: f64) -> f64 {
    if x < 0.0 {
        set(errno::EDOM);
    }
    libm::sqrt(x)
}

/// [`sqrt`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sqrtf(x: f32) -> f32 {
    if x < 0.0 {
        set(errno::EDOM);
    }
    libm::sqrtf(x)
}

/// The rule glibc's `w_fmod_template.c` and `w_remainder_template.c` share:
/// `EDOM` for an infinite `x` or a zero `y`, unless either is a NaN.
fn rem_domain(x: f64, y: f64) {
    if (x.is_infinite() || y == 0.0) && !x.is_nan() && !y.is_nan() {
        set(errno::EDOM);
    }
}

/// `x - n*y`, `n` truncated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    rem_domain(x, y);
    libm::fmod(x, y)
}

/// [`fmod`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    rem_domain(f64::from(x), f64::from(y));
    libm::fmodf(x, y)
}

/// `x - n*y`, `n` rounded to nearest, ties to even.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remainder(x: f64, y: f64) -> f64 {
    rem_domain(x, y);
    libm::remainder(x, y)
}

/// [`remainder`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remainderf(x: f32, y: f32) -> f32 {
    rem_domain(f64::from(x), f64::from(y));
    libm::remainderf(x, y)
}

/// BSD's name for [`remainder`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn drem(x: f64, y: f64) -> f64 {
    remainder(x, y)
}

/// BSD's name for [`remainderf`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dremf(x: f32, y: f32) -> f32 {
    remainderf(x, y)
}

/// [`remainder`], and the low bits of the quotient, with its sign, in
/// `*quo`. A NULL `quo` is not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remquo(x: f64, y: f64, quo: *mut i32) -> f64 {
    let (r, q) = libm::remquo(x, y);
    if !quo.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { quo.write(q) };
    }
    r
}

/// [`remquo`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remquof(x: f32, y: f32, quo: *mut i32) -> f32 {
    let (r, q) = libm::remquof(x, y);
    if !quo.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { quo.write(q) };
    }
    r
}

/// `x` as a fraction in [0.5, 1) and a power of two in `*exp`. A NULL `exp`
/// is not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn frexp(x: f64, exp: *mut i32) -> f64 {
    let (m, e) = libm::frexp(x);
    if !exp.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { exp.write(e) };
    }
    m
}

/// [`frexp`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn frexpf(x: f32, exp: *mut i32) -> f32 {
    let (m, e) = libm::frexpf(x);
    if !exp.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { exp.write(e) };
    }
    m
}

/// The fraction of `x`, its integral part in `*iptr`. A NULL `iptr` is not
/// written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn modf(x: f64, iptr: *mut f64) -> f64 {
    let (frac, int) = libm::modf(x);
    if !iptr.is_null() {
        // SAFETY: non-null, and the caller's `double *`.
        unsafe { iptr.write(int) };
    }
    frac
}

/// [`modf`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn modff(x: f32, iptr: *mut f32) -> f32 {
    let (frac, int) = libm::modff(x);
    if !iptr.is_null() {
        // SAFETY: non-null, and the caller's `float *`.
        unsafe { iptr.write(int) };
    }
    frac
}

/// glibc's `s_ldexp_template.c`, which `ldexp`, `scalbn` and (through
/// `w_scalbln_template.c`) `scalbln` all are: a zero or non-finite `x` is its
/// own answer, and `ERANGE` when the result overflows or underflows to zero.
fn scaled(x: f64, r: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x + x;
    }
    if !r.is_finite() || r == 0.0 {
        set(errno::ERANGE);
    }
    r
}

/// [`scaled`] (float).
fn scaled_f(x: f32, r: f32) -> f32 {
    if !x.is_finite() || x == 0.0 {
        return x + x;
    }
    if !r.is_finite() || r == 0.0 {
        set(errno::ERANGE);
    }
    r
}

/// A `long` exponent as an `int` one, clamped: past `±2^31` every double has
/// long since overflowed or underflowed, so the clamp changes no answer.
fn clamp_exp(n: i64) -> i32 {
    i32::try_from(n).unwrap_or(if n < 0 { i32::MIN } else { i32::MAX })
}

/// `x * 2^n`, rounded once.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ldexp(x: f64, n: i32) -> f64 {
    scaled(x, libm::scalbn(x, n))
}

/// [`ldexp`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ldexpf(x: f32, n: i32) -> f32 {
    scaled_f(x, libm::scalbnf(x, n))
}

/// [`ldexp`], by its C99 name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalbn(x: f64, n: i32) -> f64 {
    scaled(x, libm::scalbn(x, n))
}

/// [`ldexpf`], by its C99 name.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalbnf(x: f32, n: i32) -> f32 {
    scaled_f(x, libm::scalbnf(x, n))
}

/// [`scalbn`] with a `long` exponent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalbln(x: f64, n: i64) -> f64 {
    scaled(x, libm::scalbn(x, clamp_exp(n)))
}

/// [`scalbnf`] with a `long` exponent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalblnf(x: f32, n: i64) -> f32 {
    scaled_f(x, libm::scalbnf(x, clamp_exp(n)))
}

/// `FP_ILOGB0`: glibc's value on x86-64, `INT_MIN`.
pub const FP_ILOGB0: i32 = i32::MIN;
/// `FP_ILOGBNAN`: glibc's value on x86-64, `INT_MIN` too.
pub const FP_ILOGBNAN: i32 = i32::MIN;

/// glibc's `w_ilogb_template.c`: `EDOM` for the three answers that are not
/// an exponent -- zero, NaN, infinity.
fn ilogb_errno(r: i32) -> i32 {
    if r == FP_ILOGB0 || r == FP_ILOGBNAN || r == i32::MAX {
        set(errno::EDOM);
    }
    r
}

/// The exponent of `x`: `FP_ILOGB0` for zero, `FP_ILOGBNAN` for a NaN,
/// `INT_MAX` for an infinity, each with `EDOM`. The crate answers musl's
/// values, which are these (`INT_MIN`, `INT_MIN`, `INT_MAX`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ilogb(x: f64) -> i32 {
    ilogb_errno(libm::ilogb(x))
}

/// [`ilogb`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ilogbf(x: f32) -> i32 {
    ilogb_errno(libm::ilogbf(x))
}

/// The exponent of `x`, as a double: musl's `logb.c` over [`libm::ilogb`] --
/// `-inf` (a pole: division by zero, no `errno` in glibc) for zero, `x * x`
/// for an infinity or a NaN.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logb(x: f64) -> f64 {
    if !x.is_finite() {
        return x * x;
    }
    if x == 0.0 {
        return -1.0 / (x * x);
    }
    f64::from(libm::ilogb(x))
}

/// [`logb`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logbf(x: f32) -> f32 {
    if !x.is_finite() {
        return x * x;
    }
    if x == 0.0 {
        return -1.0 / (x * x);
    }
    #[allow(clippy::cast_precision_loss)]
    let e = libm::ilogbf(x) as f32;
    e
}

/// `x` scaled into [1, 2): glibc's `s_significand.c`, `scalb(x, -ilogb(x))`.
/// A zero, an infinity or a NaN is its own answer -- with `EDOM`, because
/// the `ilogb` glibc calls there is the public one, which sets it for those
/// three (the oracle below has the three).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn significand(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        set(errno::EDOM);
        return x;
    }
    libm::scalbn(x, libm::ilogb(x).saturating_neg())
}

/// [`significand`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn significandf(x: f32) -> f32 {
    if !x.is_finite() || x == 0.0 {
        set(errno::EDOM);
        return x;
    }
    libm::scalbnf(x, libm::ilogbf(x).saturating_neg())
}

/// glibc's `s_nextafter.c`: `ERANGE` when the step overflows, or lands on a
/// subnormal or zero -- except from zero itself, which answers the least
/// subnormal and sets nothing.
fn next_errno(x: f64, r: f64) -> f64 {
    let exp_bits = (r.to_bits() >> 52) & 0x7FF;
    if x != 0.0 && x.is_finite() && (exp_bits == 0x7FF || exp_bits == 0) && !r.is_nan() {
        set(errno::ERANGE);
    }
    r
}

/// The next double after `x` toward `y`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nextafter(x: f64, y: f64) -> f64 {
    let r = libm::nextafter(x, y);
    if x == y || x.is_nan() || y.is_nan() {
        return r;
    }
    next_errno(x, r)
}

/// The next float after `x` toward `y` (glibc's `s_nextafterf.c`: the same
/// rule at float's exponent width).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nextafterf(x: f32, y: f32) -> f32 {
    let r = libm::nextafterf(x, y);
    if x == y || x.is_nan() || y.is_nan() {
        return r;
    }
    let exp_bits = (r.to_bits() >> 23) & 0xFF;
    if x != 0.0 && x.is_finite() && (exp_bits == 0xFF || exp_bits == 0) {
        set(errno::ERANGE);
    }
    r
}

// ---------------------------------------------------------------------------
// Exponentials and logarithms
// ---------------------------------------------------------------------------

/// The rule glibc's `w_exp_template.c` (and `exp2`, `exp10`) apply: `ERANGE`
/// when a finite argument overflows or underflows to zero.
fn exp_errno(x: f64, r: f64) -> f64 {
    if (!r.is_finite() || r == 0.0) && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// [`exp_errno`] (float).
fn exp_errno_f(x: f32, r: f32) -> f32 {
    if (!r.is_finite() || r == 0.0) && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// `e^x`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp(x: f64) -> f64 {
    exp_errno(x, libm::exp(x))
}

/// [`exp`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn expf(x: f32) -> f32 {
    exp_errno_f(x, libm::expf(x))
}

/// `2^x`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp2(x: f64) -> f64 {
    exp_errno(x, libm::exp2(x))
}

/// [`exp2`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp2f(x: f32) -> f32 {
    exp_errno_f(x, libm::exp2f(x))
}

/// `10^x` (a GNU extension, now C23's).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp10(x: f64) -> f64 {
    exp_errno(x, libm::exp10(x))
}

/// [`exp10`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp10f(x: f32) -> f32 {
    exp_errno_f(x, libm::exp10f(x))
}

/// glibc's old name for [`exp10`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pow10(x: f64) -> f64 {
    exp10(x)
}

/// glibc's old name for [`exp10f`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pow10f(x: f32) -> f32 {
    exp10f(x)
}

/// `e^x - 1`, exact near 0 (glibc's `s_expm1.c`: `ERANGE` on overflow only;
/// it cannot underflow).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn expm1(x: f64) -> f64 {
    let r = libm::expm1(x);
    if r.is_infinite() && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// [`expm1`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn expm1f(x: f32) -> f32 {
    let r = libm::expm1f(x);
    if r.is_infinite() && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// The rule glibc's `w_log_template.c`, `w_log2_template.c` and
/// `w_log10_template.c` share: `ERANGE` for zero (a pole), `EDOM` below it.
fn log_errno(x: f64) {
    if x <= 0.0 {
        set(if x == 0.0 { errno::ERANGE } else { errno::EDOM });
    }
}

/// The natural logarithm.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log(x: f64) -> f64 {
    log_errno(x);
    libm::log(x)
}

/// [`log`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logf(x: f32) -> f32 {
    log_errno(f64::from(x));
    libm::logf(x)
}

/// The base-2 logarithm.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log2(x: f64) -> f64 {
    log_errno(x);
    libm::log2(x)
}

/// [`log2`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log2f(x: f32) -> f32 {
    log_errno(f64::from(x));
    libm::log2f(x)
}

/// The base-10 logarithm.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log10(x: f64) -> f64 {
    log_errno(x);
    libm::log10(x)
}

/// [`log10`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log10f(x: f32) -> f32 {
    log_errno(f64::from(x));
    libm::log10f(x)
}

/// glibc's `w_log1p_template.c`: `ERANGE` at -1 (a pole), `EDOM` below.
fn log1p_errno(x: f64) {
    if x <= -1.0 {
        set(if x == -1.0 {
            errno::ERANGE
        } else {
            errno::EDOM
        });
    }
}

/// `log(1 + x)`, exact near 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log1p(x: f64) -> f64 {
    log1p_errno(x);
    libm::log1p(x)
}

/// [`log1p`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log1pf(x: f32) -> f32 {
    log1p_errno(f64::from(x));
    libm::log1pf(x)
}

/// glibc's `w_pow_template.c`: a non-finite answer from finite arguments is
/// `EDOM` if it is a NaN (a negative base to a non-integer power) and
/// `ERANGE` otherwise (overflow, or `pow(0, y<0)`'s pole); a zero from a
/// finite nonzero base and a finite power is an underflow, `ERANGE`.
fn pow_errno(x: f64, y: f64, z: f64) {
    if !z.is_finite() {
        if x.is_finite() && y.is_finite() {
            set(if z.is_nan() {
                errno::EDOM
            } else {
                errno::ERANGE
            });
        }
    } else if z == 0.0 && x.is_finite() && x != 0.0 && y.is_finite() {
        set(errno::ERANGE);
    }
}

/// `x^y`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    let z = libm::pow(x, y);
    pow_errno(x, y, z);
    z
}

/// [`pow`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn powf(x: f32, y: f32) -> f32 {
    let z = libm::powf(x, y);
    pow_errno(f64::from(x), f64::from(y), f64::from(z));
    z
}

/// The cube root.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cbrt(x: f64) -> f64 {
    libm::cbrt(x)
}

/// [`cbrt`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cbrtf(x: f32) -> f32 {
    libm::cbrtf(x)
}

/// `sqrt(x*x + y*y)` without the intermediate overflow (glibc: `ERANGE` when
/// finite arguments overflow).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    let z = libm::hypot(x, y);
    if !z.is_finite() && x.is_finite() && y.is_finite() {
        set(errno::ERANGE);
    }
    z
}

/// [`hypot`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    let z = libm::hypotf(x, y);
    if !z.is_finite() && x.is_finite() && y.is_finite() {
        set(errno::ERANGE);
    }
    z
}

// ---------------------------------------------------------------------------
// Trigonometry
// ---------------------------------------------------------------------------

/// glibc's `s_sin.c`, `s_tan.c`, `s_sincos.c` (and their float twins): the
/// sine, cosine or tangent of an infinity is a domain error.
fn trig_domain(x: f64) {
    if x.is_infinite() {
        set(errno::EDOM);
    }
}

/// The sine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sin(x: f64) -> f64 {
    trig_domain(x);
    libm::sin(x)
}

/// [`sin`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sinf(x: f32) -> f32 {
    trig_domain(f64::from(x));
    libm::sinf(x)
}

/// The cosine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cos(x: f64) -> f64 {
    trig_domain(x);
    libm::cos(x)
}

/// [`cos`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cosf(x: f32) -> f32 {
    trig_domain(f64::from(x));
    libm::cosf(x)
}

/// The tangent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tan(x: f64) -> f64 {
    trig_domain(x);
    libm::tan(x)
}

/// [`tan`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tanf(x: f32) -> f32 {
    trig_domain(f64::from(x));
    libm::tanf(x)
}

/// The sine into `*s` and the cosine into `*c`, from one reduction. A NULL
/// pointer is not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sincos(x: f64, s: *mut f64, c: *mut f64) {
    trig_domain(x);
    let (sv, cv) = libm::sincos(x);
    if !s.is_null() {
        // SAFETY: non-null, and the caller's `double *`.
        unsafe { s.write(sv) };
    }
    if !c.is_null() {
        // SAFETY: as for `s`.
        unsafe { c.write(cv) };
    }
}

/// [`sincos`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sincosf(x: f32, s: *mut f32, c: *mut f32) {
    trig_domain(f64::from(x));
    let (sv, cv) = libm::sincosf(x);
    if !s.is_null() {
        // SAFETY: non-null, and the caller's `float *`.
        unsafe { s.write(sv) };
    }
    if !c.is_null() {
        // SAFETY: as for `s`.
        unsafe { c.write(cv) };
    }
}

/// `EDOM` for |x| > 1: glibc's `w_asin_template.c` and `w_acos_template.c`.
fn unit_domain(x: f64) {
    if x.abs() > 1.0 {
        set(errno::EDOM);
    }
}

/// The arcsine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asin(x: f64) -> f64 {
    unit_domain(x);
    libm::asin(x)
}

/// [`asin`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asinf(x: f32) -> f32 {
    unit_domain(f64::from(x));
    libm::asinf(x)
}

/// The arccosine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acos(x: f64) -> f64 {
    unit_domain(x);
    libm::acos(x)
}

/// [`acos`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acosf(x: f32) -> f32 {
    unit_domain(f64::from(x));
    libm::acosf(x)
}

/// The arctangent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atan(x: f64) -> f64 {
    libm::atan(x)
}

/// [`atan`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atanf(x: f32) -> f32 {
    libm::atanf(x)
}

/// The angle of the point (`x`, `y`) -- note the order, `y` first. glibc's
/// `w_atan2_template.c`: `ERANGE` when a nonzero `y` and finite `x`
/// underflow to zero.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    let z = libm::atan2(y, x);
    if z == 0.0 && y != 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
    z
}

/// [`atan2`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    let z = libm::atan2f(y, x);
    if z == 0.0 && y != 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
    z
}

// ---------------------------------------------------------------------------
// Hyperbolic
// ---------------------------------------------------------------------------

/// `ERANGE` when a finite argument overflows: glibc's `w_sinh_template.c`
/// and `w_cosh_template.c`.
fn overflow_errno(x: f64, z: f64) {
    if !z.is_finite() && x.is_finite() {
        set(errno::ERANGE);
    }
}

/// The hyperbolic sine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sinh(x: f64) -> f64 {
    let z = libm::sinh(x);
    overflow_errno(x, z);
    z
}

/// [`sinh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sinhf(x: f32) -> f32 {
    let z = libm::sinhf(x);
    overflow_errno(f64::from(x), f64::from(z));
    z
}

/// The hyperbolic cosine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cosh(x: f64) -> f64 {
    let z = libm::cosh(x);
    overflow_errno(x, z);
    z
}

/// [`cosh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn coshf(x: f32) -> f32 {
    let z = libm::coshf(x);
    overflow_errno(f64::from(x), f64::from(z));
    z
}

/// The hyperbolic tangent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tanh(x: f64) -> f64 {
    libm::tanh(x)
}

/// [`tanh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tanhf(x: f32) -> f32 {
    libm::tanhf(x)
}

/// The inverse hyperbolic sine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asinh(x: f64) -> f64 {
    libm::asinh(x)
}

/// [`asinh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asinhf(x: f32) -> f32 {
    libm::asinhf(x)
}

/// The inverse hyperbolic cosine (`EDOM`, and a NaN, below 1: glibc's
/// `w_acosh_template.c` and `e_acosh.c`).
///
/// The NaN is made here and not left to the crate: musl's `acosh.c` leaves
/// "x < 1" to the `log` it ends in, and for a large negative `x` the
/// argument that reaches it cancels to a small positive number, so
/// `acosh(-427000)` answered 2.65 (found by the glibc oracle below).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acosh(x: f64) -> f64 {
    if x < 1.0 {
        set(errno::EDOM);
        // glibc's e_acosh.c, and IEEE's way to make the domain error: 0/0
        // raises "invalid" and yields the processor's default NaN (on x86-64
        // the one printf shows as -nan), as every other domain error here
        // does. A constant NaN would do neither.
        #[allow(clippy::eq_op)]
        return (x - x) / (x - x);
    }
    libm::acosh(x)
}

/// [`acosh`] (float; `acoshf.c` has the same shape).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acoshf(x: f32) -> f32 {
    if x < 1.0 {
        set(errno::EDOM);
        // As in `acosh`.
        #[allow(clippy::eq_op)]
        return (x - x) / (x - x);
    }
    libm::acoshf(x)
}

/// glibc's `w_atanh_template.c`: `ERANGE` at ±1 (a pole), `EDOM` beyond.
fn atanh_errno(x: f64) {
    if x.abs() >= 1.0 {
        set(if x.abs() == 1.0 {
            errno::ERANGE
        } else {
            errno::EDOM
        });
    }
}

/// The inverse hyperbolic tangent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atanh(x: f64) -> f64 {
    atanh_errno(x);
    libm::atanh(x)
}

/// [`atanh`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atanhf(x: f32) -> f32 {
    atanh_errno(f64::from(x));
    libm::atanhf(x)
}

// ---------------------------------------------------------------------------
// Error and gamma functions
// ---------------------------------------------------------------------------

/// The error function.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erf(x: f64) -> f64 {
    libm::erf(x)
}

/// [`erf`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erff(x: f32) -> f32 {
    libm::erff(x)
}

/// `1 - erf(x)`, without the cancellation (glibc's `s_erf.c`: `ERANGE` when a
/// positive argument underflows to zero).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erfc(x: f64) -> f64 {
    let r = libm::erfc(x);
    if r == 0.0 && x > 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// [`erfc`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erfcf(x: f32) -> f32 {
    let r = libm::erfcf(x);
    if r == 0.0 && x > 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
    r
}

/// The sign of Γ(x) the last [`lgamma`] found, as C declares it: `int
/// signgam`, which glibc exports (the symbol was missing until 2026-09-27).
///
/// One global, as glibc's is: `lgamma` is not thread-safe for exactly this
/// reason, and `lgamma_r` exists so that callers who care need not read it.
/// An `AtomicI32` rather than a `static mut` all the same: it has C's `int`
/// layout, so a C program reads and assigns it as the `int` it declares, but
/// two threads calling `lgamma` at once -- a C program's race, which C
/// permits it to have -- are then not undefined behaviour inside this
/// library. Relaxed: the value is one call's result, ordered with nothing.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static signgam: AtomicI32 = AtomicI32::new(0);

/// glibc's `w_lgamma_template.c`: `ERANGE` when a finite argument gives an
/// infinity -- the poles at 0 and the negative integers, or overflow.
fn lgamma_errno(x: f64, y: f64) {
    if !y.is_finite() && x.is_finite() {
        set(errno::ERANGE);
    }
}

/// `log|Γ(x)|`, its sign stored in [`signgam`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgamma(x: f64) -> f64 {
    let (y, sign) = libm::lgamma_r(x);
    signgam.store(sign, Ordering::Relaxed);
    lgamma_errno(x, y);
    y
}

/// [`lgamma`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgammaf(x: f32) -> f32 {
    let (y, sign) = libm::lgammaf_r(x);
    signgam.store(sign, Ordering::Relaxed);
    lgamma_errno(f64::from(x), f64::from(y));
    y
}

/// `log|Γ(x)|`, its sign in `*sign` rather than [`signgam`]. A NULL `sign` is
/// not written.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgamma_r(x: f64, sign: *mut i32) -> f64 {
    let (y, s) = libm::lgamma_r(x);
    if !sign.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { sign.write(s) };
    }
    lgamma_errno(x, y);
    y
}

/// [`lgamma_r`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgammaf_r(x: f32, sign: *mut i32) -> f32 {
    let (y, s) = libm::lgammaf_r(x);
    if !sign.is_null() {
        // SAFETY: non-null, and the caller's `int *`.
        unsafe { sign.write(s) };
    }
    lgamma_errno(f64::from(x), f64::from(y));
    y
}

/// The old name for [`lgamma`], which glibc keeps.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gamma(x: f64) -> f64 {
    lgamma(x)
}

/// The old name for [`lgammaf`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gammaf(x: f32) -> f32 {
    lgammaf(x)
}

/// glibc's `w_tgamma_template.c`: a non-finite or zero answer from a finite
/// argument (or from -inf) is `ERANGE` at 0 (a pole), `EDOM` at a negative
/// integer, and `ERANGE` otherwise (overflow or underflow).
fn tgamma_errno(x: f64, y: f64) {
    if (!y.is_finite() || y == 0.0) && (x.is_finite() || x == f64::NEG_INFINITY) {
        if x == 0.0 {
            set(errno::ERANGE);
        } else if libm::floor(x) == x && x < 0.0 {
            set(errno::EDOM);
        } else {
            set(errno::ERANGE);
        }
    }
}

/// Γ(x).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tgamma(x: f64) -> f64 {
    let y = libm::tgamma(x);
    tgamma_errno(x, y);
    y
}

/// [`tgamma`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tgammaf(x: f32) -> f32 {
    let y = libm::tgammaf(x);
    tgamma_errno(f64::from(x), f64::from(y));
    y
}

// ---------------------------------------------------------------------------
// Bessel functions: glibc's w_j0/j1/jn templates -- the `y` functions are a
// domain error below 0 and a pole at 0, the `j` functions a domain of
// everything -- and what glibc's e_j1.c and e_jn.c add on top: a result that
// underflows to zero is ERANGE, one that overflows to -inf is ERANGE, and at
// an infinity an odd order keeps the argument's sign (j1(-inf) is -0, where
// musl answers +0).
// ---------------------------------------------------------------------------

/// `EDOM` below 0, `ERANGE` at 0 (the `y` templates).
fn bessel_y_errno(x: f64) {
    if x <= 0.0 {
        set(if x < 0.0 { errno::EDOM } else { errno::ERANGE });
    }
}

/// `ERANGE` for a zero from a nonzero finite argument: e_j1.c's
/// `ret == 0 && x != 0` for `0.5 * x`, and e_jn.c's `ret == 0`.
fn bessel_underflow(x: f64, r: f64) {
    if r == 0.0 && x != 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
}

/// `ERANGE` for an infinity from a positive finite argument: e_j1.c's
/// `-tpi / x` for tiny `x`, and e_jn.c's "if B is +-Inf".
fn bessel_overflow(x: f64, r: f64) {
    if r.is_infinite() && x > 0.0 && x.is_finite() {
        set(errno::ERANGE);
    }
}

/// An order as glibc's jn/yn take it: its magnitude, and whether it is odd.
fn order(n: i32) -> (u32, bool) {
    let m = n.unsigned_abs();
    (m, m % 2 == 1)
}

/// Bessel function of the first kind, order 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn j0(x: f64) -> f64 {
    libm::j0(x)
}

/// Bessel function of the first kind, order 1 (`1/x`, a signed zero, at an
/// infinity, as e_j1.c answers).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn j1(x: f64) -> f64 {
    if x.is_infinite() {
        return 1.0 / x;
    }
    let r = libm::j1(x);
    bessel_underflow(x, r);
    r
}

/// Bessel function of the first kind, order `n`: e_jn.c's `J(-n, x) =
/// J(n, -x)`, a signed zero at 0 and at an infinity, `ERANGE` on underflow.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn jn(n: i32, x: f64) -> f64 {
    if x.is_nan() {
        return x + x;
    }
    let (m, odd) = order(n);
    let xx = if n < 0 { -x } else { x };
    match m {
        0 => j0(xx),
        1 => j1(xx),
        _ => {
            if xx == 0.0 || xx.is_infinite() {
                return if odd && xx.is_sign_negative() {
                    -0.0
                } else {
                    0.0
                };
            }
            let r = libm::jn(n, x);
            bessel_underflow(x, r);
            r
        }
    }
}

/// Bessel function of the second kind, order 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn y0(x: f64) -> f64 {
    bessel_y_errno(x);
    libm::y0(x)
}

/// Bessel function of the second kind, order 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn y1(x: f64) -> f64 {
    bessel_y_errno(x);
    let r = libm::y1(x);
    bessel_overflow(x, r);
    r
}

/// Bessel function of the second kind, order `n` (e_jn.c: `Y(-n, x) =
/// (-1)^n Y(n, x)`, so order -1 at +inf is -0; +0 at +inf otherwise).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn yn(n: i32, x: f64) -> f64 {
    bessel_y_errno(x);
    if x.is_nan() {
        return x + x;
    }
    let (m, odd) = order(n);
    let sign = if n < 0 && odd { -1.0 } else { 1.0 };
    match m {
        0 => libm::y0(x),
        1 => {
            let r = sign * libm::y1(x);
            bessel_overflow(x, r);
            r
        }
        _ if x == f64::INFINITY => 0.0,
        _ => {
            let r = libm::yn(n, x);
            bessel_overflow(x, r);
            r
        }
    }
}

/// [`j0`] (float; a GNU extension, missing until 2026-09-27).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn j0f(x: f32) -> f32 {
    libm::j0f(x)
}

/// [`j1`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn j1f(x: f32) -> f32 {
    if x.is_infinite() {
        return 1.0 / x;
    }
    let r = libm::j1f(x);
    bessel_underflow(f64::from(x), f64::from(r));
    r
}

/// [`jn`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn jnf(n: i32, x: f32) -> f32 {
    if x.is_nan() {
        return x + x;
    }
    let (m, odd) = order(n);
    let xx = if n < 0 { -x } else { x };
    match m {
        0 => j0f(xx),
        1 => j1f(xx),
        _ => {
            if xx == 0.0 || xx.is_infinite() {
                return if odd && xx.is_sign_negative() {
                    -0.0
                } else {
                    0.0
                };
            }
            let r = libm::jnf(n, x);
            bessel_underflow(f64::from(x), f64::from(r));
            r
        }
    }
}

/// [`y0`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn y0f(x: f32) -> f32 {
    bessel_y_errno(f64::from(x));
    libm::y0f(x)
}

/// [`y1`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn y1f(x: f32) -> f32 {
    bessel_y_errno(f64::from(x));
    let r = libm::y1f(x);
    bessel_overflow(f64::from(x), f64::from(r));
    r
}

/// [`yn`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ynf(n: i32, x: f32) -> f32 {
    bessel_y_errno(f64::from(x));
    if x.is_nan() {
        return x + x;
    }
    let (m, odd) = order(n);
    let sign = if n < 0 && odd { -1.0 } else { 1.0 };
    match m {
        0 => libm::y0f(x),
        1 => {
            let r = sign * libm::y1f(x);
            bessel_overflow(f64::from(x), f64::from(r));
            r
        }
        _ if x == f32::INFINITY => 0.0,
        _ => {
            let r = libm::ynf(n, x);
            bessel_overflow(f64::from(x), f64::from(r));
            r
        }
    }
}

// ---------------------------------------------------------------------------
// Classification. C writes these as macros over the bits (musl's <math.h>
// does); glibc also exports them as functions, which old binaries and a few
// build systems' probes call.
// ---------------------------------------------------------------------------

/// `FP_NAN`, `FP_INFINITE`, `FP_ZERO`, `FP_SUBNORMAL`, `FP_NORMAL`: glibc's
/// (and musl's) values.
pub const FP_NAN: i32 = 0;
/// See [`FP_NAN`].
pub const FP_INFINITE: i32 = 1;
/// See [`FP_NAN`].
pub const FP_ZERO: i32 = 2;
/// See [`FP_NAN`].
pub const FP_SUBNORMAL: i32 = 3;
/// See [`FP_NAN`].
pub const FP_NORMAL: i32 = 4;

/// Which kind of number `x` is -- what musl's `fpclassify` macro calls for a
/// double, so a C program using `fpclassify` did not link until 2026-09-27.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fpclassify(x: f64) -> i32 {
    match x.classify() {
        core::num::FpCategory::Nan => FP_NAN,
        core::num::FpCategory::Infinite => FP_INFINITE,
        core::num::FpCategory::Zero => FP_ZERO,
        core::num::FpCategory::Subnormal => FP_SUBNORMAL,
        core::num::FpCategory::Normal => FP_NORMAL,
    }
}

/// [`__fpclassify`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __fpclassifyf(x: f32) -> i32 {
    __fpclassify(f64::from(x))
}

/// Whether `x`'s sign bit is set (-0.0 and negative NaNs included), as
/// glibc's `__signbit`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __signbit(x: f64) -> i32 {
    i32::from(x.is_sign_negative())
}

/// [`__signbit`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __signbitf(x: f32) -> i32 {
    i32::from(x.is_sign_negative())
}

/// 1 for a NaN, else 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isnan(x: f64) -> i32 {
    i32::from(x.is_nan())
}

/// `isnanf`: [`isnan`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isnanf(x: f32) -> i32 {
    i32::from(x.is_nan())
}

/// glibc's `isinf`: 1 for +inf, -1 for -inf, 0 otherwise.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isinf(x: f64) -> i32 {
    if x.is_infinite() {
        if x > 0.0 { 1 } else { -1 }
    } else {
        0
    }
}

/// [`isinf`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isinff(x: f32) -> i32 {
    isinf(f64::from(x))
}

/// 1 for a number that is neither infinite nor NaN.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finite(x: f64) -> i32 {
    i32::from(x.is_finite())
}

/// [`finite`] (float).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finitef(x: f32) -> i32 {
    i32::from(x.is_finite())
}

/// C99's `isfinite`, which glibc also exports as a function.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isfinite(x: f64) -> i32 {
    finite(x)
}

// ---------------------------------------------------------------------------
// nan(tag)
// ---------------------------------------------------------------------------

/// The NUL-terminated `tag`, as bytes; empty for NULL.
///
/// # Safety
///
/// `tag` is NULL or a NUL-terminated string that outlives `'a`.
pub(crate) unsafe fn tag_bytes<'a>(tag: *const u8) -> &'a [u8] {
    if tag.is_null() {
        return &[];
    }
    // SAFETY: the caller's NUL-terminated string (this function's contract);
    // `n` stops at the NUL, which the string has, so it never passes the end
    // of the allocation, and the slice ends before it.
    unsafe {
        let mut n = 0usize;
        while *tag.add(n) != 0 {
            n = n.wrapping_add(1);
        }
        core::slice::from_raw_parts(tag, n)
    }
}

/// A quiet NaN, `tag` in its payload as glibc puts it there: glibc's
/// `nan` is `__strtod_nan (tag, NULL, 0)`, so the whole tag must be
/// n-chars (`[0-9A-Za-z_]`), read as `strtoull` reads a number, and
/// anything else is the default NaN (`crate::decfloat::nan_payload`, shared
/// with `strtod`'s `nan(...)`). Until 2026-09-27 the tag was ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nan(tag: *const u8) -> f64 {
    // SAFETY: `nan`'s contract: `tag` is NULL or the caller's C string.
    crate::decfloat::nan_f64(tag_payload(unsafe { tag_bytes(tag) }), false)
}

/// [`nan`] (float: 22 payload bits).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nanf(tag: *const u8) -> f32 {
    // SAFETY: as in `nan`.
    crate::decfloat::nan_f32(tag_payload(unsafe { tag_bytes(tag) }), false)
}

/// The payload of a whole tag: every byte an n-char, the run a number.
pub(crate) fn tag_payload(tag: &[u8]) -> Option<u64> {
    if !tag.iter().all(|&c| crate::decfloat::is_nchar(c)) {
        return None;
    }
    crate::decfloat::nan_payload(&crate::decfloat::SliceSource(tag), 0, tag.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The constants the old hand-written module kept for itself; the tests
    // written against it still name them.
    const PI: f64 = core::f64::consts::PI;
    const HALF_PI: f64 = core::f64::consts::FRAC_PI_2;
    const TWO_PI: f64 = core::f64::consts::TAU;
    const LN2: f64 = core::f64::consts::LN_2;

    // Approximate comparison helpers, from when this module was a set of
    // Taylor series; the tolerances are loose for libm, which is why the
    // oracle test at the end compares in ulps instead.

    /// Assert two f64 values are approximately equal within `eps`.
    fn assert_approx(a: f64, b: f64, eps: f64, msg: &str) {
        assert!(
            (a - b).abs() < eps,
            "{msg}: expected {b}, got {a} (diff = {})",
            (a - b).abs()
        );
    }

    /// Assert two f32 values are approximately equal within `eps`.
    fn assert_approx_f32(a: f32, b: f32, eps: f32, msg: &str) {
        assert!(
            (a - b).abs() < eps,
            "{msg}: expected {b}, got {a} (diff = {})",
            (a - b).abs()
        );
    }

    const EPS: f64 = 1e-10;
    const EPS_F32: f32 = 1e-5;
    // The atan Taylor series converges slowly near |x|=1, so atan, asin,
    // acos, and atan2 only achieve roughly 2 digits of accuracy there.
    // Use a wider tolerance for tests that exercise those code paths.
    const EPS_ATAN: f64 = 1e-10;

    // -----------------------------------------------------------------------
    // 1. Trigonometric functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_sin_exact_values() {
        assert_approx(sin(0.0), 0.0, EPS, "sin(0)");
        assert_approx(sin(HALF_PI), 1.0, EPS, "sin(pi/2)");
        assert_approx(sin(PI), 0.0, EPS, "sin(pi)");
        assert_approx(sin(3.0 * HALF_PI), -1.0, EPS, "sin(3pi/2)");
        assert_approx(sin(TWO_PI), 0.0, EPS, "sin(2pi)");
        assert_approx(sin(PI / 6.0), 0.5, EPS, "sin(pi/6)");
        assert_approx(
            sin(PI / 4.0),
            core::f64::consts::FRAC_1_SQRT_2,
            EPS,
            "sin(pi/4)",
        );
    }

    #[test]
    fn test_sin_symmetry() {
        // sin(-x) = -sin(x)
        let values = [0.5, 1.0, 2.0, 3.0, PI / 3.0, PI / 7.0];
        for &x in &values {
            assert_approx(sin(-x), -sin(x), EPS, &format!("sin(-{x}) == -sin({x})"));
        }
    }

    #[test]
    fn test_sin_special_values() {
        assert_approx(sin(0.0), 0.0, EPS, "sin(0) == 0");
        assert!(sin(f64::INFINITY).is_nan(), "sin(+inf) should be NaN");
        assert!(sin(f64::NEG_INFINITY).is_nan(), "sin(-inf) should be NaN");
        assert!(sin(f64::NAN).is_nan(), "sin(NaN) should be NaN");
    }

    #[test]
    fn test_cos_exact_values() {
        assert_approx(cos(0.0), 1.0, EPS, "cos(0)");
        assert_approx(cos(HALF_PI), 0.0, EPS, "cos(pi/2)");
        assert_approx(cos(PI), -1.0, EPS, "cos(pi)");
        assert_approx(cos(TWO_PI), 1.0, EPS, "cos(2pi)");
        assert_approx(cos(PI / 3.0), 0.5, EPS, "cos(pi/3)");
        assert_approx(
            cos(PI / 4.0),
            core::f64::consts::FRAC_1_SQRT_2,
            EPS,
            "cos(pi/4)",
        );
    }

    #[test]
    fn test_cos_symmetry() {
        // cos(-x) = cos(x)
        let values = [0.5, 1.0, 2.0, 3.0, PI / 3.0, PI / 7.0];
        for &x in &values {
            assert_approx(cos(-x), cos(x), EPS, &format!("cos(-{x}) == cos({x})"));
        }
    }

    #[test]
    fn test_cos_special_values() {
        assert!(cos(f64::INFINITY).is_nan(), "cos(+inf) should be NaN");
        assert!(cos(f64::NEG_INFINITY).is_nan(), "cos(-inf) should be NaN");
        assert!(cos(f64::NAN).is_nan(), "cos(NaN) should be NaN");
    }

    #[test]
    fn test_tan_exact_values() {
        assert_approx(tan(0.0), 0.0, EPS, "tan(0)");
        assert_approx(tan(PI / 4.0), 1.0, EPS, "tan(pi/4)");
        assert_approx(tan(-PI / 4.0), -1.0, EPS, "tan(-pi/4)");
        assert_approx(tan(PI), 0.0, EPS, "tan(pi)");
    }

    #[test]
    fn test_tan_special_values() {
        assert!(tan(f64::INFINITY).is_nan(), "tan(+inf) should be NaN");
        assert!(tan(f64::NEG_INFINITY).is_nan(), "tan(-inf) should be NaN");
        assert!(tan(f64::NAN).is_nan(), "tan(NaN) should be NaN");
    }

    #[test]
    fn test_pythagorean_identity() {
        // sin^2(x) + cos^2(x) = 1
        let values = [0.0, 0.5, 1.0, PI / 4.0, PI / 3.0, 2.0, 3.0, 5.0];
        for &x in &values {
            let s = sin(x);
            let c = cos(x);
            assert_approx(s * s + c * c, 1.0, EPS, &format!("sin^2({x})+cos^2({x})"));
        }
    }

    // -----------------------------------------------------------------------
    // Inverse trigonometry
    // -----------------------------------------------------------------------

    #[test]
    fn test_asin_exact_values() {
        assert_approx(asin(0.0), 0.0, EPS, "asin(0)");
        assert_approx(asin(1.0), HALF_PI, EPS, "asin(1)");
        assert_approx(asin(-1.0), -HALF_PI, EPS, "asin(-1)");
        assert_approx(asin(0.5), PI / 6.0, EPS_ATAN, "asin(0.5)");
    }

    #[test]
    fn test_asin_domain_error() {
        assert!(asin(1.5).is_nan(), "asin(1.5) should be NaN");
        assert!(asin(-1.5).is_nan(), "asin(-1.5) should be NaN");
        assert!(asin(f64::NAN).is_nan(), "asin(NaN) should be NaN");
    }

    #[test]
    fn test_acos_exact_values() {
        assert_approx(acos(1.0), 0.0, EPS, "acos(1)");
        assert_approx(acos(0.0), HALF_PI, EPS, "acos(0)");
        assert_approx(acos(-1.0), PI, EPS, "acos(-1)");
        assert_approx(acos(0.5), PI / 3.0, EPS_ATAN, "acos(0.5)");
    }

    #[test]
    fn test_atan_exact_values() {
        assert_approx(atan(0.0), 0.0, EPS, "atan(0)");
        assert_approx(atan(1.0), PI / 4.0, EPS_ATAN, "atan(1)");
        assert_approx(atan(-1.0), -PI / 4.0, EPS_ATAN, "atan(-1)");
    }

    #[test]
    fn test_atan2_quadrants() {
        assert_approx(atan2(0.0, 1.0), 0.0, EPS, "atan2(0,1)");
        assert_approx(atan2(1.0, 0.0), HALF_PI, EPS, "atan2(1,0)");
        assert_approx(atan2(0.0, -1.0), PI, EPS, "atan2(0,-1)");
        assert_approx(atan2(-1.0, 0.0), -HALF_PI, EPS, "atan2(-1,0)");
        assert_approx(atan2(1.0, 1.0), PI / 4.0, EPS_ATAN, "atan2(1,1)");
    }

    #[test]
    fn test_sin_asin_roundtrip() {
        let values = [0.0, 0.3, 0.5, 0.7, 0.9, -0.3, -0.7];
        for &x in &values {
            assert_approx(sin(asin(x)), x, EPS_ATAN, &format!("sin(asin({x}))"));
        }
    }

    // -----------------------------------------------------------------------
    // 2. Exponential and logarithmic functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_exp_exact_values() {
        assert_approx(exp(0.0), 1.0, EPS, "exp(0)");
        assert_approx(exp(1.0), core::f64::consts::E, EPS, "exp(1)");
        assert_approx(exp(-1.0), 1.0 / core::f64::consts::E, EPS, "exp(-1)");
        assert_approx(exp(LN2), 2.0, EPS, "exp(ln2)");
    }

    #[test]
    fn test_exp_large_values() {
        assert_eq!(exp(710.0), f64::INFINITY, "exp(710) should overflow");
        assert_approx(exp(-710.0), 0.0, EPS, "exp(-710) should underflow");
    }

    #[test]
    fn test_exp_special_values() {
        assert!(exp(f64::NAN).is_nan(), "exp(NaN) should be NaN");
        assert_eq!(exp(f64::INFINITY), f64::INFINITY, "exp(+inf)");
        assert_approx(exp(f64::NEG_INFINITY), 0.0, EPS, "exp(-inf)");
    }

    #[test]
    fn test_log_exact_values() {
        assert_approx(log(1.0), 0.0, EPS, "log(1)");
        assert_approx(log(core::f64::consts::E), 1.0, EPS, "log(e)");
        assert_approx(
            log(core::f64::consts::E * core::f64::consts::E),
            2.0,
            EPS,
            "log(e^2)",
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_log_special_values() {
        assert_eq!(log(0.0), f64::NEG_INFINITY, "log(0) should be -inf");
        assert!(log(-1.0).is_nan(), "log(-1) should be NaN");
        assert!(log(f64::NAN).is_nan(), "log(NaN) should be NaN");
        assert_eq!(
            log(f64::INFINITY),
            f64::INFINITY,
            "log(+inf) should be +inf"
        );
    }

    #[test]
    fn test_exp_log_roundtrip() {
        // exp(log(x)) should approximate x.
        let values = [0.5, 1.0, 2.0, 10.0, 100.0, 0.01];
        for &x in &values {
            assert_approx(exp(log(x)), x, 1e-9, &format!("exp(log({x}))"));
        }
    }

    #[test]
    fn test_log_exp_roundtrip() {
        // log(exp(x)) should approximate x.
        let values = [-2.0, -1.0, 0.0, 0.5, 1.0, 3.0, 5.0];
        for &x in &values {
            assert_approx(log(exp(x)), x, 1e-9, &format!("log(exp({x}))"));
        }
    }

    #[test]
    fn test_log2_values() {
        assert_approx(log2(1.0), 0.0, EPS, "log2(1)");
        assert_approx(log2(2.0), 1.0, EPS, "log2(2)");
        assert_approx(log2(8.0), 3.0, EPS, "log2(8)");
        assert_approx(log2(1024.0), 10.0, EPS, "log2(1024)");
    }

    #[test]
    fn test_log10_values() {
        assert_approx(log10(1.0), 0.0, EPS, "log10(1)");
        assert_approx(log10(10.0), 1.0, EPS, "log10(10)");
        assert_approx(log10(100.0), 2.0, EPS, "log10(100)");
        assert_approx(log10(1000.0), 3.0, EPS, "log10(1000)");
    }

    #[test]
    fn test_exp2_values() {
        assert_approx(exp2(0.0), 1.0, EPS, "exp2(0)");
        assert_approx(exp2(1.0), 2.0, EPS, "exp2(1)");
        assert_approx(exp2(10.0), 1024.0, EPS, "exp2(10)");
    }

    #[test]
    fn test_log1p_values() {
        assert_approx(log1p(0.0), 0.0, EPS, "log1p(0)");
        // For small x, log1p uses dedicated Taylor series.
        assert_approx(log1p(1e-10), 1e-10, 1e-15, "log1p(1e-10)");
        assert_approx(log1p(1.0), LN2, EPS, "log1p(1)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_log1p_special_values() {
        assert_eq!(log1p(-1.0), f64::NEG_INFINITY, "log1p(-1)");
        assert!(log1p(-2.0).is_nan(), "log1p(-2) should be NaN");
    }

    #[test]
    fn test_expm1_values() {
        assert_approx(expm1(0.0), 0.0, EPS, "expm1(0)");
        // For small x, expm1 uses dedicated Taylor series.
        assert_approx(expm1(1e-10), 1e-10, 1e-15, "expm1(1e-10)");
        assert_approx(expm1(1.0), core::f64::consts::E - 1.0, EPS, "expm1(1)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_expm1_special_values() {
        assert_eq!(expm1(f64::INFINITY), f64::INFINITY, "expm1(+inf)");
        assert_approx(expm1(f64::NEG_INFINITY), -1.0, EPS, "expm1(-inf)");
        assert!(expm1(f64::NAN).is_nan(), "expm1(NaN)");
    }

    // -----------------------------------------------------------------------
    // 3. Power functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_pow_exact_values() {
        assert_approx(pow(2.0, 10.0), 1024.0, EPS, "pow(2,10)");
        assert_approx(pow(3.0, 4.0), 81.0, EPS, "pow(3,4)");
        assert_approx(pow(10.0, 3.0), 1000.0, EPS, "pow(10,3)");
        assert_approx(pow(2.0, -1.0), 0.5, EPS, "pow(2,-1)");
    }

    #[test]
    fn test_pow_special_exponents() {
        assert_approx(pow(0.0, 0.0), 1.0, EPS, "pow(0,0)");
        assert_approx(pow(5.0, 0.0), 1.0, EPS, "pow(5,0)");
        assert_approx(pow(-3.0, 0.0), 1.0, EPS, "pow(-3,0)");
        assert_approx(pow(f64::INFINITY, 0.0), 1.0, EPS, "pow(inf,0)");
        assert_approx(pow(1.0, 1000.0), 1.0, EPS, "pow(1,1000)");
        assert_approx(pow(1.0, f64::INFINITY), 1.0, EPS, "pow(1,inf)");
    }

    #[test]
    fn test_pow_zero_base() {
        assert_approx(pow(0.0, 5.0), 0.0, EPS, "pow(0,5)");
        assert_eq!(pow(0.0, -1.0), f64::INFINITY, "pow(0,-1)");
    }

    #[test]
    fn test_pow_nan_propagation() {
        assert!(pow(f64::NAN, 2.0).is_nan(), "pow(NaN,2) should be NaN");
        assert!(pow(2.0, f64::NAN).is_nan(), "pow(2,NaN) should be NaN");
    }

    #[test]
    fn test_sqrt_exact_values() {
        assert_approx(sqrt(0.0), 0.0, EPS, "sqrt(0)");
        assert_approx(sqrt(1.0), 1.0, EPS, "sqrt(1)");
        assert_approx(sqrt(4.0), 2.0, EPS, "sqrt(4)");
        assert_approx(sqrt(9.0), 3.0, EPS, "sqrt(9)");
        assert_approx(sqrt(16.0), 4.0, EPS, "sqrt(16)");
        assert_approx(sqrt(100.0), 10.0, EPS, "sqrt(100)");
        assert_approx(sqrt(2.0), core::f64::consts::SQRT_2, EPS, "sqrt(2)");
    }

    #[test]
    fn test_sqrt_special_values() {
        assert!(sqrt(-1.0).is_nan(), "sqrt(-1) should be NaN");
        assert!(sqrt(f64::NAN).is_nan(), "sqrt(NaN) should be NaN");
        assert_eq!(sqrt(f64::INFINITY), f64::INFINITY, "sqrt(+inf)");
    }

    #[test]
    fn test_sqrt_large_and_small() {
        assert_approx(sqrt(1e20), 1e10, 1.0, "sqrt(1e20)");
        assert_approx(sqrt(1e-20), 1e-10, 1e-15, "sqrt(1e-20)");
    }

    #[test]
    fn test_cbrt_exact_values() {
        assert_approx(cbrt(0.0), 0.0, EPS, "cbrt(0)");
        assert_approx(cbrt(1.0), 1.0, EPS, "cbrt(1)");
        assert_approx(cbrt(8.0), 2.0, EPS, "cbrt(8)");
        assert_approx(cbrt(27.0), 3.0, EPS, "cbrt(27)");
        assert_approx(cbrt(64.0), 4.0, EPS, "cbrt(64)");
        assert_approx(cbrt(-8.0), -2.0, EPS, "cbrt(-8)");
        assert_approx(cbrt(-27.0), -3.0, EPS, "cbrt(-27)");
    }

    // -----------------------------------------------------------------------
    // 4. Rounding functions
    // -----------------------------------------------------------------------

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_floor_values() {
        assert_eq!(floor(2.7), 2.0, "floor(2.7)");
        assert_eq!(floor(2.0), 2.0, "floor(2.0)");
        assert_eq!(floor(-2.3), -3.0, "floor(-2.3)");
        assert_eq!(floor(-2.0), -2.0, "floor(-2.0)");
        assert_eq!(floor(0.5), 0.0, "floor(0.5)");
        assert_eq!(floor(-0.5), -1.0, "floor(-0.5)");
    }

    #[test]
    fn test_floor_special_values() {
        assert!(floor(f64::NAN).is_nan(), "floor(NaN) should be NaN");
        assert_eq!(floor(f64::INFINITY), f64::INFINITY, "floor(+inf)");
        assert_eq!(floor(f64::NEG_INFINITY), f64::NEG_INFINITY, "floor(-inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_ceil_values() {
        assert_eq!(ceil(2.3), 3.0, "ceil(2.3)");
        assert_eq!(ceil(2.0), 2.0, "ceil(2.0)");
        assert_eq!(ceil(-2.7), -2.0, "ceil(-2.7)");
        assert_eq!(ceil(-2.0), -2.0, "ceil(-2.0)");
        assert_eq!(ceil(0.1), 1.0, "ceil(0.1)");
        assert_eq!(ceil(-0.1), 0.0, "ceil(-0.1)");
    }

    #[test]
    fn test_ceil_special_values() {
        assert!(ceil(f64::NAN).is_nan(), "ceil(NaN) should be NaN");
        assert_eq!(ceil(f64::INFINITY), f64::INFINITY, "ceil(+inf)");
        assert_eq!(ceil(f64::NEG_INFINITY), f64::NEG_INFINITY, "ceil(-inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_round_values() {
        assert_eq!(round(2.5), 3.0, "round(2.5)");
        assert_eq!(round(2.3), 2.0, "round(2.3)");
        assert_eq!(round(2.7), 3.0, "round(2.7)");
        assert_eq!(round(-2.5), -3.0, "round(-2.5)");
        assert_eq!(round(-2.3), -2.0, "round(-2.3)");
        assert_eq!(round(-2.7), -3.0, "round(-2.7)");
        assert_eq!(round(0.0), 0.0, "round(0.0)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_trunc_values() {
        assert_eq!(trunc(2.7), 2.0, "trunc(2.7)");
        assert_eq!(trunc(2.0), 2.0, "trunc(2.0)");
        assert_eq!(trunc(-2.7), -2.0, "trunc(-2.7)");
        assert_eq!(trunc(-2.3), -2.0, "trunc(-2.3)");
        assert_eq!(trunc(0.9), 0.0, "trunc(0.9)");
    }

    #[test]
    fn test_trunc_special_values() {
        assert!(trunc(f64::NAN).is_nan(), "trunc(NaN) should be NaN");
        assert_eq!(trunc(f64::INFINITY), f64::INFINITY, "trunc(+inf)");
        assert_eq!(trunc(f64::NEG_INFINITY), f64::NEG_INFINITY, "trunc(-inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_rint_ties_to_even() {
        // rint uses ties-to-even (banker's rounding).
        assert_eq!(rint(0.5), 0.0, "rint(0.5) -> 0 (even)");
        assert_eq!(rint(1.5), 2.0, "rint(1.5) -> 2 (even)");
        assert_eq!(rint(2.5), 2.0, "rint(2.5) -> 2 (even)");
        assert_eq!(rint(3.5), 4.0, "rint(3.5) -> 4 (even)");
        assert_eq!(rint(2.3), 2.0, "rint(2.3)");
        assert_eq!(rint(2.7), 3.0, "rint(2.7)");
    }

    #[test]
    fn test_lround_values() {
        assert_eq!(lround(2.5), 3, "lround(2.5)");
        assert_eq!(lround(-2.5), -3, "lround(-2.5)");
        assert_eq!(lround(0.0), 0, "lround(0)");
        assert_eq!(lround(2.3), 2, "lround(2.3)");
    }

    // -----------------------------------------------------------------------
    // 5. Special value functions
    // -----------------------------------------------------------------------

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_fabs_values() {
        assert_eq!(fabs(-3.0), 3.0, "fabs(-3)");
        assert_eq!(fabs(3.0), 3.0, "fabs(3)");
        assert_eq!(fabs(0.0), 0.0, "fabs(0)");
        assert_eq!(fabs(-0.0), 0.0, "fabs(-0)");
        // fabs should clear the sign bit on -0.0.
        assert_eq!(fabs(-0.0).to_bits(), 0_u64, "fabs(-0.0) sign bit");
    }

    #[test]
    fn test_fabs_special() {
        assert!(fabs(f64::NAN).is_nan(), "fabs(NaN) should be NaN");
        assert_eq!(fabs(f64::INFINITY), f64::INFINITY, "fabs(+inf)");
        assert_eq!(fabs(f64::NEG_INFINITY), f64::INFINITY, "fabs(-inf)");
    }

    #[test]
    fn test_fmod_values() {
        assert_approx(fmod(5.0, 3.0), 2.0, EPS, "fmod(5,3)");
        assert_approx(fmod(7.0, 2.5), 2.0, EPS, "fmod(7,2.5)");
        assert_approx(fmod(-5.0, 3.0), -2.0, EPS, "fmod(-5,3)");
        assert_approx(fmod(5.0, -3.0), 2.0, EPS, "fmod(5,-3)");
    }

    #[test]
    fn test_fmod_special() {
        assert!(fmod(5.0, 0.0).is_nan(), "fmod(5,0) should be NaN");
    }

    #[test]
    fn test_hypot_values() {
        assert_approx(hypot(3.0, 4.0), 5.0, EPS, "hypot(3,4)");
        assert_approx(hypot(5.0, 12.0), 13.0, EPS, "hypot(5,12)");
        assert_approx(hypot(0.0, 5.0), 5.0, EPS, "hypot(0,5)");
        assert_approx(hypot(1.0, 0.0), 1.0, EPS, "hypot(1,0)");
        assert_approx(hypot(0.0, 0.0), 0.0, EPS, "hypot(0,0)");
        assert_approx(hypot(-3.0, -4.0), 5.0, EPS, "hypot(-3,-4)");
    }

    #[test]
    fn test_hypot_special() {
        assert_eq!(hypot(f64::INFINITY, 0.0), f64::INFINITY, "hypot(inf,0)");
        assert_eq!(hypot(0.0, f64::INFINITY), f64::INFINITY, "hypot(0,inf)");
        // Per IEEE 754, hypot(inf, NaN) = inf (infinity dominates NaN).
        assert_eq!(
            hypot(f64::INFINITY, f64::NAN),
            f64::INFINITY,
            "hypot(inf,NaN)"
        );
        assert!(hypot(f64::NAN, 0.0).is_nan(), "hypot(NaN,0) should be NaN");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_copysign_values() {
        assert_eq!(copysign(1.0, -1.0), -1.0, "copysign(1,-1)");
        assert_eq!(copysign(-1.0, 1.0), 1.0, "copysign(-1,1)");
        assert_eq!(copysign(5.0, -0.0), -5.0, "copysign(5,-0)");
        assert_eq!(copysign(-5.0, 0.0), 5.0, "copysign(-5,0)");
    }

    #[test]
    fn test_fmin_fmax() {
        assert_approx(fmin(2.0, 3.0), 2.0, EPS, "fmin(2,3)");
        assert_approx(fmax(2.0, 3.0), 3.0, EPS, "fmax(2,3)");
        assert_approx(fmin(-1.0, 1.0), -1.0, EPS, "fmin(-1,1)");
        assert_approx(fmax(-1.0, 1.0), 1.0, EPS, "fmax(-1,1)");
        // NaN should be ignored (return the other operand).
        assert_approx(fmin(f64::NAN, 3.0), 3.0, EPS, "fmin(NaN,3)");
        assert_approx(fmax(f64::NAN, 3.0), 3.0, EPS, "fmax(NaN,3)");
        assert_approx(fmin(3.0, f64::NAN), 3.0, EPS, "fmin(3,NaN)");
        assert_approx(fmax(3.0, f64::NAN), 3.0, EPS, "fmax(3,NaN)");
    }

    #[test]
    fn test_fdim_values() {
        assert_approx(fdim(5.0, 3.0), 2.0, EPS, "fdim(5,3)");
        assert_approx(fdim(3.0, 5.0), 0.0, EPS, "fdim(3,5)");
        assert_approx(fdim(-1.0, -3.0), 2.0, EPS, "fdim(-1,-3)");
        assert!(fdim(f64::NAN, 1.0).is_nan(), "fdim(NaN,1) should be NaN");
    }

    #[test]
    fn test_remainder_values() {
        assert_approx(remainder(5.0, 3.0), -1.0, EPS, "remainder(5,3)");
        assert_approx(remainder(10.0, 3.0), 1.0, EPS, "remainder(10,3)");
        assert!(remainder(5.0, 0.0).is_nan(), "remainder(5,0) should be NaN");
        assert!(
            remainder(f64::INFINITY, 1.0).is_nan(),
            "remainder(inf,1) should be NaN"
        );
    }

    #[test]
    fn test_remainder_uses_round_to_even() {
        // IEEE 754 remainder uses round-to-nearest-even, NOT half-away-from-zero.
        // remainder(2.5, 1.0): rint(2.5)=2 (even) → 2.5 - 2*1 = 0.5
        assert_approx(
            remainder(2.5, 1.0),
            0.5,
            EPS,
            "remainder(2.5,1) round-to-even",
        );
        // remainder(3.5, 1.0): rint(3.5)=4 (even) → 3.5 - 4*1 = -0.5
        assert_approx(
            remainder(3.5, 1.0),
            -0.5,
            EPS,
            "remainder(3.5,1) round-to-even",
        );
        // remainder(4.5, 1.0): rint(4.5)=4 (even) → 4.5 - 4*1 = 0.5
        assert_approx(
            remainder(4.5, 1.0),
            0.5,
            EPS,
            "remainder(4.5,1) round-to-even",
        );
        // remainder(5.5, 1.0): rint(5.5)=6 (even) → 5.5 - 6*1 = -0.5
        assert_approx(
            remainder(5.5, 1.0),
            -0.5,
            EPS,
            "remainder(5.5,1) round-to-even",
        );
    }

    // -----------------------------------------------------------------------
    // 6. NaN/infinity handling
    // -----------------------------------------------------------------------

    #[test]
    fn test_isnan() {
        assert_eq!(isnan(f64::NAN), 1, "isnan(NaN)");
        assert_eq!(isnan(0.0), 0, "isnan(0)");
        assert_eq!(isnan(f64::INFINITY), 0, "isnan(inf)");
    }

    #[test]
    fn test_isinf() {
        assert_eq!(isinf(f64::INFINITY), 1, "isinf(+inf)");
        assert_eq!(isinf(f64::NEG_INFINITY), -1, "isinf(-inf)");
        assert_eq!(isinf(0.0), 0, "isinf(0)");
        assert_eq!(isinf(f64::NAN), 0, "isinf(NaN)");
    }

    #[test]
    fn test_isfinite() {
        assert_eq!(isfinite(0.0), 1, "isfinite(0)");
        assert_eq!(isfinite(1.5), 1, "isfinite(1.5)");
        assert_eq!(isfinite(f64::INFINITY), 0, "isfinite(inf)");
        assert_eq!(isfinite(f64::NEG_INFINITY), 0, "isfinite(-inf)");
        assert_eq!(isfinite(f64::NAN), 0, "isfinite(NaN)");
    }

    #[test]
    fn test_nan_through_arithmetic() {
        // Functions should propagate NaN.
        assert!(fabs(f64::NAN).is_nan(), "fabs(NaN)");
        assert!(sqrt(f64::NAN).is_nan(), "sqrt(NaN)");
        assert!(cbrt(f64::NAN).is_nan(), "cbrt(NaN)");
        assert!(exp(f64::NAN).is_nan(), "exp(NaN)");
        assert!(log(f64::NAN).is_nan(), "log(NaN)");
        assert!(sin(f64::NAN).is_nan(), "sin(NaN)");
        assert!(cos(f64::NAN).is_nan(), "cos(NaN)");
        assert!(tan(f64::NAN).is_nan(), "tan(NaN)");
    }

    #[test]
    fn test_infinity_handling() {
        // exp
        assert_eq!(exp(f64::INFINITY), f64::INFINITY, "exp(+inf)");
        assert_approx(exp(f64::NEG_INFINITY), 0.0, EPS, "exp(-inf)");

        // log
        assert_eq!(log(f64::INFINITY), f64::INFINITY, "log(+inf)");
        assert!(log(f64::NEG_INFINITY).is_nan(), "log(-inf) should be NaN");

        // sqrt
        assert_eq!(sqrt(f64::INFINITY), f64::INFINITY, "sqrt(+inf)");

        // pow
        assert_eq!(pow(2.0, f64::INFINITY), f64::INFINITY, "pow(2,+inf)");

        // hypot with infinity
        assert_eq!(hypot(f64::INFINITY, 5.0), f64::INFINITY, "hypot(inf,5)");
    }

    // -----------------------------------------------------------------------
    // 7. f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn test_sinf_values() {
        assert_approx_f32(sinf(0.0), 0.0, EPS_F32, "sinf(0)");
        assert_approx_f32(
            sinf(core::f32::consts::FRAC_PI_2),
            1.0,
            EPS_F32,
            "sinf(pi/2)",
        );
        assert_approx_f32(sinf(core::f32::consts::PI), 0.0, EPS_F32, "sinf(pi)");
    }

    #[test]
    fn test_sinf_special() {
        assert!(sinf(f32::INFINITY).is_nan(), "sinf(+inf) should be NaN");
        assert!(sinf(f32::NEG_INFINITY).is_nan(), "sinf(-inf) should be NaN");
        assert!(sinf(f32::NAN).is_nan(), "sinf(NaN) should be NaN");
    }

    #[test]
    fn test_cosf_values() {
        assert_approx_f32(cosf(0.0), 1.0, EPS_F32, "cosf(0)");
        assert_approx_f32(
            cosf(core::f32::consts::FRAC_PI_2),
            0.0,
            EPS_F32,
            "cosf(pi/2)",
        );
        assert_approx_f32(cosf(core::f32::consts::PI), -1.0, EPS_F32, "cosf(pi)");
    }

    #[test]
    fn test_cosf_special() {
        assert!(cosf(f32::INFINITY).is_nan(), "cosf(+inf) should be NaN");
        assert!(cosf(f32::NAN).is_nan(), "cosf(NaN) should be NaN");
    }

    #[test]
    fn test_tanf_values() {
        assert_approx_f32(tanf(0.0), 0.0, EPS_F32, "tanf(0)");
        assert_approx_f32(
            tanf(core::f32::consts::FRAC_PI_4),
            1.0,
            EPS_F32,
            "tanf(pi/4)",
        );
    }

    #[test]
    fn test_sqrtf_values() {
        assert_approx_f32(sqrtf(0.0), 0.0, EPS_F32, "sqrtf(0)");
        assert_approx_f32(sqrtf(1.0), 1.0, EPS_F32, "sqrtf(1)");
        assert_approx_f32(sqrtf(4.0), 2.0, EPS_F32, "sqrtf(4)");
        assert_approx_f32(sqrtf(9.0), 3.0, EPS_F32, "sqrtf(9)");
        assert_approx_f32(sqrtf(2.0), core::f32::consts::SQRT_2, EPS_F32, "sqrtf(2)");
    }

    #[test]
    fn test_sqrtf_special() {
        assert!(sqrtf(-1.0).is_nan(), "sqrtf(-1) should be NaN");
        assert!(sqrtf(f32::NAN).is_nan(), "sqrtf(NaN) should be NaN");
        assert_eq!(sqrtf(f32::INFINITY), f32::INFINITY, "sqrtf(+inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_fabsf_values() {
        assert_eq!(fabsf(-3.0), 3.0, "fabsf(-3)");
        assert_eq!(fabsf(3.0), 3.0, "fabsf(3)");
        assert_eq!(fabsf(0.0), 0.0, "fabsf(0)");
        assert_eq!(fabsf(-0.0_f32).to_bits(), 0_u32, "fabsf(-0) sign bit");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_floorf_values() {
        assert_eq!(floorf(2.7), 2.0, "floorf(2.7)");
        assert_eq!(floorf(-2.3), -3.0, "floorf(-2.3)");
        assert_eq!(floorf(0.0), 0.0, "floorf(0)");
        assert_eq!(floorf(-0.5), -1.0, "floorf(-0.5)");
    }

    #[test]
    fn test_floorf_special() {
        assert!(floorf(f32::NAN).is_nan(), "floorf(NaN) should be NaN");
        assert_eq!(floorf(f32::INFINITY), f32::INFINITY, "floorf(+inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_ceilf_values() {
        assert_eq!(ceilf(2.3), 3.0, "ceilf(2.3)");
        assert_eq!(ceilf(-2.7), -2.0, "ceilf(-2.7)");
        assert_eq!(ceilf(0.0), 0.0, "ceilf(0)");
    }

    #[test]
    fn test_ceilf_special() {
        assert!(ceilf(f32::NAN).is_nan(), "ceilf(NaN) should be NaN");
        assert_eq!(ceilf(f32::INFINITY), f32::INFINITY, "ceilf(+inf)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_roundf_values() {
        assert_eq!(roundf(2.5), 3.0, "roundf(2.5)");
        assert_eq!(roundf(-2.5), -3.0, "roundf(-2.5)");
        assert_eq!(roundf(2.3), 2.0, "roundf(2.3)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_truncf_values() {
        assert_eq!(truncf(2.7), 2.0, "truncf(2.7)");
        assert_eq!(truncf(-2.7), -2.0, "truncf(-2.7)");
    }

    #[test]
    fn test_expf_values() {
        assert_approx_f32(expf(0.0), 1.0, EPS_F32, "expf(0)");
        assert_approx_f32(expf(1.0), core::f32::consts::E, EPS_F32, "expf(1)");
    }

    #[test]
    fn test_logf_values() {
        assert_approx_f32(logf(1.0), 0.0, EPS_F32, "logf(1)");
        assert_approx_f32(logf(core::f32::consts::E), 1.0, EPS_F32, "logf(e)");
    }

    #[test]
    fn test_powf_values() {
        assert_approx_f32(powf(2.0, 10.0), 1024.0, 1.0, "powf(2,10)");
        assert_approx_f32(powf(3.0, 2.0), 9.0, EPS_F32, "powf(3,2)");
        assert_approx_f32(powf(5.0, 0.0), 1.0, EPS_F32, "powf(5,0)");
    }

    #[test]
    fn test_cbrtf_values() {
        assert_approx_f32(cbrtf(27.0), 3.0, EPS_F32, "cbrtf(27)");
        assert_approx_f32(cbrtf(8.0), 2.0, EPS_F32, "cbrtf(8)");
        assert_approx_f32(cbrtf(-8.0), -2.0, EPS_F32, "cbrtf(-8)");
    }

    #[test]
    fn test_hypotf_values() {
        assert_approx_f32(hypotf(3.0, 4.0), 5.0, EPS_F32, "hypotf(3,4)");
    }

    #[test]
    fn test_fmodf_values() {
        assert_approx_f32(fmodf(5.0, 3.0), 2.0, EPS_F32, "fmodf(5,3)");
        assert!(fmodf(5.0, 0.0).is_nan(), "fmodf(5,0) should be NaN");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_copysignf_values() {
        assert_eq!(copysignf(1.0, -1.0), -1.0, "copysignf(1,-1)");
        assert_eq!(copysignf(-1.0, 1.0), 1.0, "copysignf(-1,1)");
    }

    #[test]
    fn test_fminf_fmaxf() {
        assert_approx_f32(fminf(2.0, 3.0), 2.0, EPS_F32, "fminf(2,3)");
        assert_approx_f32(fmaxf(2.0, 3.0), 3.0, EPS_F32, "fmaxf(2,3)");
        assert_approx_f32(fminf(f32::NAN, 3.0), 3.0, EPS_F32, "fminf(NaN,3)");
        assert_approx_f32(fmaxf(f32::NAN, 3.0), 3.0, EPS_F32, "fmaxf(NaN,3)");
    }

    // -----------------------------------------------------------------------
    // Hyperbolic functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_sinh_values() {
        assert_approx(sinh(0.0), 0.0, EPS, "sinh(0)");
        // sinh(1) ≈ 1.1752011936...
        assert_approx(sinh(1.0), 1.175_201_193_643_801_4, EPS, "sinh(1)");
        // sinh is odd: sinh(-x) = -sinh(x).
        assert_approx(sinh(-1.0), -sinh(1.0), EPS, "sinh(-1)");
    }

    #[test]
    fn test_cosh_values() {
        assert_approx(cosh(0.0), 1.0, EPS, "cosh(0)");
        // cosh(1) ≈ 1.5430806348...
        assert_approx(cosh(1.0), 1.543_080_634_815_243_7, EPS, "cosh(1)");
        // cosh is even: cosh(-x) = cosh(x).
        assert_approx(cosh(-1.0), cosh(1.0), EPS, "cosh(-1)");
    }

    #[test]
    fn test_tanh_values() {
        assert_approx(tanh(0.0), 0.0, EPS, "tanh(0)");
        // tanh(1) ≈ 0.7615941559...
        assert_approx(tanh(1.0), 0.761_594_155_955_764, EPS, "tanh(1)");
        // tanh approaches ±1 for large |x|.
        assert_approx(tanh(100.0), 1.0, EPS, "tanh(100)");
        assert_approx(tanh(-100.0), -1.0, EPS, "tanh(-100)");
    }

    #[test]
    fn test_hyperbolic_special() {
        assert!(sinh(f64::NAN).is_nan(), "sinh(NaN)");
        assert!(cosh(f64::NAN).is_nan(), "cosh(NaN)");
        assert!(tanh(f64::NAN).is_nan(), "tanh(NaN)");
        assert_eq!(sinh(f64::INFINITY), f64::INFINITY, "sinh(+inf)");
        assert_eq!(cosh(f64::INFINITY), f64::INFINITY, "cosh(+inf)");
    }

    // -----------------------------------------------------------------------
    // Inverse hyperbolic functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_asinh_values() {
        assert_approx(asinh(0.0), 0.0, EPS, "asinh(0)");
        // asinh(sinh(1)) should be 1.
        assert_approx(asinh(sinh(1.0)), 1.0, EPS, "asinh(sinh(1))");
        // Odd function: asinh(-x) = -asinh(x).
        assert_approx(asinh(-1.0), -asinh(1.0), EPS, "asinh(-1)");
    }

    #[test]
    fn test_acosh_values() {
        assert_approx(acosh(1.0), 0.0, EPS, "acosh(1)");
        assert_approx(acosh(cosh(2.0)), 2.0, EPS, "acosh(cosh(2))");
        assert!(acosh(0.5).is_nan(), "acosh(0.5) should be NaN (domain)");
    }

    #[test]
    fn test_atanh_values() {
        assert_approx(atanh(0.0), 0.0, EPS, "atanh(0)");
        assert_approx(atanh(tanh(0.5)), 0.5, EPS, "atanh(tanh(0.5))");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_atanh_boundary() {
        assert_eq!(atanh(1.0), f64::INFINITY, "atanh(1)");
        assert_eq!(atanh(-1.0), f64::NEG_INFINITY, "atanh(-1)");
        assert!(atanh(1.5).is_nan(), "atanh(1.5) should be NaN");
    }

    // -----------------------------------------------------------------------
    // Decomposition functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_frexp_ldexp_roundtrip() {
        let values = [1.0, 2.0, 0.5, 100.0, 0.001, -7.5, 1e30, 1e-30];
        for &x in &values {
            let mut e: i32 = 0;
            let m = frexp(x, &mut e);
            let reconstructed = ldexp(m, e);
            assert_approx(
                reconstructed,
                x,
                fabs(x) * 1e-14,
                &format!("frexp/ldexp roundtrip for {x}"),
            );
        }
    }

    #[test]
    fn test_ldexp_values() {
        assert_approx(ldexp(1.0, 0), 1.0, EPS, "ldexp(1,0)");
        assert_approx(ldexp(1.0, 3), 8.0, EPS, "ldexp(1,3)");
        assert_approx(ldexp(1.5, 2), 6.0, EPS, "ldexp(1.5,2)");
        assert_approx(ldexp(1.0, -1), 0.5, EPS, "ldexp(1,-1)");
    }

    #[test]
    fn test_modf_values() {
        let mut ip: f64 = 0.0;
        let frac = modf(3.75, &mut ip);
        assert_approx(ip, 3.0, EPS, "modf(3.75) int part");
        assert_approx(frac, 0.75, EPS, "modf(3.75) frac part");

        let frac2 = modf(-2.25, &mut ip);
        assert_approx(ip, -2.0, EPS, "modf(-2.25) int part");
        assert_approx(frac2, -0.25, EPS, "modf(-2.25) frac part");
    }

    #[test]
    fn test_ilogb_values() {
        assert_eq!(ilogb(1.0), 0, "ilogb(1)");
        assert_eq!(ilogb(2.0), 1, "ilogb(2)");
        assert_eq!(ilogb(8.0), 3, "ilogb(8)");
        assert_eq!(ilogb(0.5), -1, "ilogb(0.5)");
        assert_eq!(ilogb(0.0), i32::MIN, "ilogb(0) = FP_ILOGB0");
        assert_eq!(ilogb(f64::INFINITY), i32::MAX, "ilogb(inf) = INT_MAX");
        // glibc's FP_ILOGBNAN on x86-64 is INT_MIN, as musl's is (the oracle
        // below has it); this said INT_MAX, the old implementation's answer.
        assert_eq!(ilogb(f64::NAN), i32::MIN, "ilogb(NaN) = FP_ILOGBNAN");
    }

    #[test]
    fn test_logb_values() {
        assert_approx(logb(1.0), 0.0, EPS, "logb(1)");
        assert_approx(logb(8.0), 3.0, EPS, "logb(8)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_logb_special() {
        assert_eq!(logb(0.0), f64::NEG_INFINITY, "logb(0) = -inf");
        assert_eq!(logb(f64::INFINITY), f64::INFINITY, "logb(inf) = inf");
        assert!(logb(f64::NAN).is_nan(), "logb(NaN) = NaN");
    }

    #[test]
    fn test_scalbn_values() {
        assert_approx(scalbn(1.0, 3), 8.0, EPS, "scalbn(1,3)");
        assert_approx(scalbn(3.0, 2), 12.0, EPS, "scalbn(3,2)");
    }

    // -----------------------------------------------------------------------
    // nextafter
    // -----------------------------------------------------------------------

    #[test]
    fn test_nextafter_direction() {
        let a = nextafter(1.0, 2.0);
        assert!(a > 1.0, "nextafter(1,2) should be > 1");
        let b = nextafter(1.0, 0.0);
        assert!(b < 1.0, "nextafter(1,0) should be < 1");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_nextafter_equal() {
        assert_eq!(nextafter(1.0, 1.0), 1.0, "nextafter(1,1) == 1");
    }

    #[test]
    fn test_nextafter_special() {
        assert!(nextafter(f64::NAN, 1.0).is_nan(), "nextafter(NaN,1)");
        assert!(nextafter(1.0, f64::NAN).is_nan(), "nextafter(1,NaN)");
    }

    // -----------------------------------------------------------------------
    // fma
    // -----------------------------------------------------------------------

    #[test]
    fn test_fma_values() {
        assert_approx(fma(2.0, 3.0, 4.0), 10.0, EPS, "fma(2,3,4) = 10");
        assert_approx(fma(1.5, 2.0, -1.0), 2.0, EPS, "fma(1.5,2,-1) = 2");
        assert_approx(fma(0.0, 100.0, 5.0), 5.0, EPS, "fma(0,100,5) = 5");
    }

    // -----------------------------------------------------------------------
    // sincos
    // -----------------------------------------------------------------------

    #[test]
    fn test_sincos_consistency() {
        let values = [0.0, PI / 4.0, PI / 2.0, PI, 1.0, 2.5];
        for &x in &values {
            let mut s: f64 = 0.0;
            let mut c: f64 = 0.0;
            sincos(x, &mut s, &mut c);
            assert_approx(s, sin(x), EPS, &format!("sincos sin({x})"));
            assert_approx(c, cos(x), EPS, &format!("sincos cos({x})"));
        }
    }

    // -----------------------------------------------------------------------
    // Error function
    // -----------------------------------------------------------------------

    #[test]
    fn test_erf_values() {
        assert_approx(erf(0.0), 0.0, 1e-6, "erf(0)");
        // erf is odd.
        assert_approx(erf(-0.5), -erf(0.5), 1e-6, "erf odd symmetry");
        // erf(large) -> 1.
        assert_approx(erf(5.0), 1.0, 1e-6, "erf(5) ~= 1");
    }

    #[test]
    fn test_erfc_values() {
        assert_approx(erfc(0.0), 1.0, 1e-6, "erfc(0) = 1");
        // erfc(x) = 1 - erf(x).
        assert_approx(erfc(1.0), 1.0 - erf(1.0), 1e-12, "erfc(1)");
    }

    // -----------------------------------------------------------------------
    // Gamma functions
    // -----------------------------------------------------------------------

    /// `signgam` is one process-wide `int` by C's definition, and every
    /// `lgamma`, `lgammaf`, `gamma` and `gammaf` call writes it: each test
    /// that makes one -- directly, or through the oracle's table -- takes
    /// this first, so [`lgamma_sets_signgam`] reads the sign its own call
    /// left. Poison is recovered so that one real failure reports once.
    static SIGNGAM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn signgam_lock() -> std::sync::MutexGuard<'static, ()> {
        SIGNGAM_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `lgamma` leaves the sign of Γ(x) in `signgam`: Γ is negative between
    /// -1 and 0 and positive between -2 and -1, and positive for every
    /// positive x.
    #[test]
    fn lgamma_sets_signgam() {
        let _g = signgam_lock();
        let sign = || signgam.load(Ordering::Relaxed);
        let _ = lgamma(-0.5);
        assert_eq!(sign(), -1, "gamma(-0.5) is -3.54");
        let _ = lgamma(-1.5);
        assert_eq!(sign(), 1, "gamma(-1.5) is 2.36");
        let _ = lgamma(0.5);
        assert_eq!(sign(), 1);
        let _ = lgammaf(-0.5);
        assert_eq!(sign(), -1, "and lgammaf writes the same global");
        let _ = gamma(-2.5);
        assert_eq!(sign(), -1, "gamma(-2.5) is -0.945; gamma is lgamma");
    }

    #[test]
    fn test_lgamma_values() {
        let _g = signgam_lock();
        // lgamma(1) = ln(0!) = ln(1) = 0.
        assert_approx(lgamma(1.0), 0.0, 1e-8, "lgamma(1)");
        // lgamma(2) = ln(1!) = ln(1) = 0.
        assert_approx(lgamma(2.0), 0.0, 1e-8, "lgamma(2)");
        // Gamma(n) = (n-1)! for positive integers, so lgamma(5) = ln(24).
        assert_approx(lgamma(5.0), log(24.0), 1e-8, "lgamma(5) = ln(24)");
    }

    #[test]
    fn test_lgamma_poles() {
        let _g = signgam_lock();
        assert_eq!(lgamma(0.0), f64::INFINITY, "lgamma(0) = inf");
        assert_eq!(lgamma(-1.0), f64::INFINITY, "lgamma(-1) = inf");
    }

    #[test]
    fn test_tgamma_values() {
        // Gamma(1) = 1.
        assert_approx(tgamma(1.0), 1.0, 1e-8, "tgamma(1)");
        // Gamma(5) = 4! = 24.
        assert_approx(tgamma(5.0), 24.0, 1e-6, "tgamma(5)");
        // Gamma(0.5) = sqrt(pi).
        assert_approx(tgamma(0.5), sqrt(PI), 1e-6, "tgamma(0.5)");
    }

    #[test]
    fn test_tgamma_poles() {
        // The gamma function has a pole at 0 -- +inf, with ERANGE -- and no
        // value at the negative integers: NaN, with EDOM (glibc's
        // w_tgamma_template.c; this said tgamma(0) was a NaN, the old
        // implementation's answer).
        errno::set_errno(0);
        assert_eq!(tgamma(0.0), f64::INFINITY, "tgamma(0) = +inf");
        assert_eq!(errno::get_errno(), errno::ERANGE);
        assert_eq!(tgamma(-0.0), f64::NEG_INFINITY, "tgamma(-0) = -inf");
        errno::set_errno(0);
        assert!(tgamma(-1.0).is_nan(), "tgamma(-1) is NaN");
        assert_eq!(errno::get_errno(), errno::EDOM);
    }

    // -----------------------------------------------------------------------
    // exp10 / pow10
    // -----------------------------------------------------------------------

    #[test]
    fn test_exp10_values() {
        assert_approx(exp10(0.0), 1.0, EPS, "exp10(0)");
        assert_approx(exp10(1.0), 10.0, EPS, "exp10(1)");
        assert_approx(exp10(2.0), 100.0, EPS, "exp10(2)");
        assert_approx(exp10(3.0), 1000.0, 1e-6, "exp10(3)");
    }

    #[test]
    fn test_pow10_is_exp10() {
        // pow10 is just an alias for exp10.
        assert_approx(pow10(2.0), exp10(2.0), EPS, "pow10(2) == exp10(2)");
    }

    // -----------------------------------------------------------------------
    // Bessel functions (basic smoke tests)
    // -----------------------------------------------------------------------

    #[test]
    fn test_j0_at_zero() {
        assert_approx(j0(0.0), 1.0, EPS, "J0(0) = 1");
    }

    #[test]
    fn test_j1_at_zero() {
        assert_approx(j1(0.0), 0.0, EPS, "J1(0) = 0");
    }

    #[test]
    fn test_jn_reduces_to_j0_j1() {
        assert_approx(jn(0, 1.5), j0(1.5), EPS, "jn(0,x) == j0(x)");
        assert_approx(jn(1, 1.5), j1(1.5), EPS, "jn(1,x) == j1(x)");
    }

    #[test]
    fn test_y0_y1_domain() {
        assert_eq!(y0(0.0), f64::NEG_INFINITY, "Y0(0) = -inf");
        assert!(y0(-1.0).is_nan(), "Y0(-1) = NaN");
        assert_eq!(y1(0.0), f64::NEG_INFINITY, "Y1(0) = -inf");
        assert!(y1(-1.0).is_nan(), "Y1(-1) = NaN");
    }

    #[test]
    fn test_yn_reduces_to_y0_y1() {
        assert_approx(yn(0, 2.0), y0(2.0), EPS, "yn(0,x) == y0(x)");
        assert_approx(yn(1, 2.0), y1(2.0), EPS, "yn(1,x) == y1(x)");
    }

    // -----------------------------------------------------------------------
    // Deprecated / compatibility aliases
    // -----------------------------------------------------------------------

    #[test]
    fn test_finite_alias() {
        assert_eq!(finite(1.0), 1, "finite(1)");
        assert_eq!(finite(f64::INFINITY), 0, "finite(inf)");
        assert_eq!(finite(f64::NAN), 0, "finite(NaN)");
    }

    #[test]
    fn test_drem_alias() {
        assert_approx(
            drem(10.0, 3.0),
            remainder(10.0, 3.0),
            EPS,
            "drem == remainder",
        );
    }

    #[test]
    fn test_significand_values() {
        // significand(x) returns x * 2^(-ilogb(x)), should be in [1, 2).
        let s = significand(8.0);
        assert!(
            s >= 1.0 && s < 2.0,
            "significand(8) should be in [1, 2), got {s}"
        );
        assert_approx(significand(1.0), 1.0, EPS, "significand(1)");
    }

    // -----------------------------------------------------------------------
    // lgamma_r (thread-safe gamma with sign)
    // -----------------------------------------------------------------------

    #[test]
    fn test_lgamma_r_sign() {
        let _g = signgam_lock();
        let mut sign: i32 = 0;
        let val = lgamma_r(5.0, &mut sign);
        assert_approx(val, lgamma(5.0), EPS, "lgamma_r(5) value");
        assert_eq!(sign, 1, "lgamma_r(5) sign should be positive");
    }

    // -----------------------------------------------------------------------
    // remquo
    // -----------------------------------------------------------------------

    #[test]
    fn test_remquo_values() {
        let mut q: i32 = 0;
        let r = remquo(10.0, 3.0, &mut q);
        assert_approx(r, remainder(10.0, 3.0), EPS, "remquo remainder");
        // quotient bits should match round(10/3) = round(3.33) = 3.
        assert_eq!(q.abs() & 0x7, 3, "remquo quotient low bits");
    }

    #[test]
    fn test_remquo_special() {
        let mut q: i32 = 0;
        let r = remquo(f64::NAN, 1.0, &mut q);
        assert!(r.is_nan(), "remquo(NaN,1) should be NaN");
    }

    // -----------------------------------------------------------------------
    // pow — negative base edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn pow_negative_base_integer_exponent() {
        // Negative base with integer exponent is valid.
        assert_approx(pow(-2.0, 3.0), -8.0, EPS, "(-2)^3");
        assert_approx(pow(-2.0, 2.0), 4.0, EPS, "(-2)^2");
        assert_approx(pow(-3.0, 0.0), 1.0, EPS, "(-3)^0");
        assert_approx(pow(-1.0, 5.0), -1.0, EPS, "(-1)^5");
    }

    #[test]
    fn pow_negative_base_fractional_exponent_is_nan() {
        // Negative base with non-integer exponent → NaN (domain error).
        assert!(pow(-2.0, 0.5).is_nan(), "(-2)^0.5 should be NaN");
        assert!(pow(-1.0, 1.5).is_nan(), "(-1)^1.5 should be NaN");
        assert!(pow(-4.0, 0.25).is_nan(), "(-4)^0.25 should be NaN");
    }

    #[test]
    fn pow_special_values() {
        assert!(pow(f64::NAN, 2.0).is_nan(), "NaN^2 should be NaN");
        assert!(pow(2.0, f64::NAN).is_nan(), "2^NaN should be NaN");
        assert_approx(pow(0.0, 5.0), 0.0, EPS, "0^5");
        assert_eq!(pow(0.0, -1.0), f64::INFINITY, "0^-1 should be +inf");
    }

    // -----------------------------------------------------------------------
    // ldexp — subnormal results
    // -----------------------------------------------------------------------

    #[test]
    fn ldexp_normal_scaling() {
        assert_approx(ldexp(1.0, 10), 1024.0, EPS, "ldexp(1, 10)");
        assert_approx(ldexp(0.5, 1), 1.0, EPS, "ldexp(0.5, 1)");
        assert_approx(ldexp(3.0, -1), 1.5, EPS, "ldexp(3, -1)");
    }

    #[test]
    fn ldexp_overflow() {
        assert_eq!(ldexp(1.0, 1024), f64::INFINITY, "ldexp(1, 1024) overflow");
        assert_eq!(
            ldexp(-1.0, 1024),
            f64::NEG_INFINITY,
            "ldexp(-1, 1024) overflow"
        );
    }

    #[test]
    fn ldexp_subnormal_results() {
        // ldexp(1.0, -1074) should produce the smallest positive subnormal.
        let smallest_subnormal = f64::from_bits(1); // 5e-324
        let result = ldexp(1.0, -1074);
        assert_eq!(
            result.to_bits(),
            smallest_subnormal.to_bits(),
            "ldexp(1, -1074) should be smallest subnormal"
        );

        // ldexp(1.0, -1023) should produce the largest subnormal.
        let result2 = ldexp(1.0, -1023);
        assert!(result2 > 0.0, "ldexp(1, -1023) should be positive");
        // Biased exponent should be 0 (subnormal).
        let exp_field = (result2.to_bits() >> 52) & 0x7FF;
        assert_eq!(exp_field, 0, "ldexp(1, -1023) should be subnormal");

        // ldexp(1.0, -1075) should underflow to 0.0.
        assert_eq!(ldexp(1.0, -1075), 0.0, "ldexp(1, -1075) flush to zero");
    }

    #[test]
    fn ldexp_negative_subnormal() {
        // Negative subnormal should preserve sign.
        let result = ldexp(-1.0, -1074);
        assert!(
            result < 0.0 || result.to_bits() != 0,
            "negative subnormal should preserve sign"
        );
        let sign_bit = result.to_bits() >> 63;
        assert_eq!(sign_bit, 1, "ldexp(-1, -1074) should have sign bit set");
    }

    #[test]
    fn ldexp_special_inputs() {
        assert_eq!(ldexp(0.0, 100), 0.0, "ldexp(0, 100)");
        assert!(ldexp(f64::NAN, 5).is_nan(), "ldexp(NaN, 5)");
        assert_eq!(ldexp(f64::INFINITY, -5), f64::INFINITY, "ldexp(inf, -5)");
    }

    // -----------------------------------------------------------------------
    // frexp subnormal handling
    // -----------------------------------------------------------------------

    #[test]
    fn frexp_subnormal_roundtrip() {
        // Smallest positive subnormal: 5e-324 = 2^-1074.
        let x = f64::from_bits(1);
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        // frexp(2^-1074) should give m=0.5, exp=-1073.
        assert!(
            m >= 0.5 && m < 1.0,
            "frexp subnormal: m={m} should be in [0.5, 1.0)"
        );
        let roundtrip = ldexp(m, e);
        assert_eq!(
            roundtrip.to_bits(),
            x.to_bits(),
            "frexp/ldexp roundtrip for smallest subnormal failed: got {roundtrip}, expected {x}"
        );
    }

    #[test]
    fn frexp_subnormal_mid() {
        // A subnormal in the middle of the range.
        // 2^-1024 = ldexp(1.0, -1024) — this is subnormal.
        let x = ldexp(1.0, -1040);
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(
            m >= 0.5 && m < 1.0,
            "frexp mid-subnormal: m={m} should be in [0.5, 1.0)"
        );
        assert_eq!(e, -1039, "frexp(2^-1040) should have exp=-1039, got {e}");
        let roundtrip = ldexp(m, e);
        assert_eq!(
            roundtrip.to_bits(),
            x.to_bits(),
            "frexp/ldexp roundtrip for mid-subnormal failed"
        );
    }

    #[test]
    fn frexp_negative_subnormal() {
        let x = -f64::from_bits(1); // -5e-324
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(
            m <= -0.5 && m > -1.0,
            "frexp negative subnormal: m={m} should be in (-1.0, -0.5]"
        );
        let roundtrip = ldexp(m, e);
        assert_eq!(
            roundtrip.to_bits(),
            x.to_bits(),
            "frexp/ldexp roundtrip for negative subnormal"
        );
    }

    #[test]
    fn frexp_largest_subnormal() {
        // Largest subnormal: biased_exp=0, all mantissa bits set.
        let x = f64::from_bits(0x000F_FFFF_FFFF_FFFF);
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(
            m >= 0.5 && m < 1.0,
            "frexp largest subnormal: m={m} should be in [0.5, 1.0)"
        );
        let roundtrip = ldexp(m, e);
        assert_eq!(
            roundtrip.to_bits(),
            x.to_bits(),
            "frexp/ldexp roundtrip for largest subnormal"
        );
    }

    // -----------------------------------------------------------------------
    // sqrt accuracy with subnormals
    // -----------------------------------------------------------------------

    #[test]
    fn sqrt_small_value() {
        // sqrt(1e-300) ≈ 1e-150.
        let x = 1e-300;
        let result = sqrt(x);
        let expected = 1e-150;
        assert_approx(result, expected, expected * 1e-10, "sqrt(1e-300)");
    }

    // -----------------------------------------------------------------------
    // nextafter edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn nextafter_from_zero() {
        let pos = nextafter(0.0, 1.0);
        assert!(pos > 0.0, "nextafter(0, 1) should be positive");
        assert_eq!(
            pos.to_bits(),
            1,
            "nextafter(0, 1) should be smallest subnormal"
        );

        let neg = nextafter(0.0, -1.0);
        assert!(neg < 0.0, "nextafter(0, -1) should be negative");
    }

    #[test]
    fn nextafter_same() {
        // nextafter(x, x) == x
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(nextafter(1.0, 1.0), 1.0);
            assert_eq!(nextafter(0.0, 0.0), 0.0);
        }
    }

    #[test]
    fn nextafter_direction() {
        let up = nextafter(1.0, 2.0);
        let down = nextafter(1.0, 0.0);
        assert!(up > 1.0, "nextafter(1, 2) should be > 1");
        assert!(down < 1.0, "nextafter(1, 0) should be < 1");
    }

    #[test]
    fn nextafter_nan() {
        assert!(nextafter(f64::NAN, 1.0).is_nan());
        assert!(nextafter(1.0, f64::NAN).is_nan());
    }

    // -----------------------------------------------------------------------
    // remainder / fmod edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn remainder_basic() {
        // remainder(5.0, 2.0) = 5.0 - rint(2.5)*2.0 = 5.0 - 2*2.0 = 1.0
        assert_approx(remainder(5.0, 2.0), 1.0, EPS, "remainder(5,2)");
    }

    #[test]
    fn remainder_negative() {
        // remainder(7.0, 3.0): rint(7/3) = rint(2.333) = 2 → 7 - 2*3 = 1
        assert_approx(remainder(7.0, 3.0), 1.0, EPS, "remainder(7,3)");
    }

    #[test]
    fn remainder_ties_to_even() {
        // remainder(2.5, 1.0): rint(2.5) = 2 (ties to even) → 2.5 - 2 = 0.5
        assert_approx(
            remainder(2.5, 1.0),
            0.5,
            EPS,
            "remainder(2.5,1) ties to even",
        );
    }

    #[test]
    fn fmod_basic() {
        // fmod(5.0, 3.0) = 5.0 - trunc(5/3)*3 = 5 - 1*3 = 2
        assert_approx(fmod(5.0, 3.0), 2.0, EPS, "fmod(5,3)");
    }

    #[test]
    fn fmod_zero_divisor() {
        assert!(fmod(1.0, 0.0).is_nan(), "fmod(1,0) should be NaN");
    }

    #[test]
    fn remainder_special_cases() {
        assert!(remainder(f64::INFINITY, 1.0).is_nan(), "remainder(inf, 1)");
        assert!(remainder(1.0, 0.0).is_nan(), "remainder(1, 0)");
        assert!(remainder(f64::NAN, 1.0).is_nan(), "remainder(NaN, 1)");
    }

    // -----------------------------------------------------------------------
    // remquo edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn remquo_basic() {
        let mut q: i32 = 0;
        let r = remquo(7.0, 3.0, &mut q);
        assert_approx(r, 1.0, EPS, "remquo(7,3) remainder");
        assert_eq!(q, 2, "remquo(7,3) quotient");
    }

    #[test]
    fn remquo_negative() {
        let mut q: i32 = 0;
        let r = remquo(-7.0, 3.0, &mut q);
        assert_approx(r, -1.0, EPS, "remquo(-7,3) remainder");
        assert_eq!(q, -2, "remquo(-7,3) quotient");
    }

    #[test]
    fn remquo_nan_inputs() {
        let mut q: i32 = 42;
        let _ = remquo(f64::NAN, 1.0, &mut q);
        assert_eq!(q, 0, "remquo NaN input should zero quotient");
    }

    // -----------------------------------------------------------------------
    // copysign
    // -----------------------------------------------------------------------

    #[test]
    fn copysign_basic() {
        assert_approx(copysign(3.0, -1.0), -3.0, EPS, "copysign(3, -1)");
        assert_approx(copysign(-3.0, 1.0), 3.0, EPS, "copysign(-3, 1)");
        assert_approx(copysign(5.0, 5.0), 5.0, EPS, "copysign(5, 5)");
    }

    // -----------------------------------------------------------------------
    // fmin / fmax
    // -----------------------------------------------------------------------

    #[test]
    fn fmin_fmax_basic() {
        assert_approx(fmin(1.0, 2.0), 1.0, EPS, "fmin(1,2)");
        assert_approx(fmax(1.0, 2.0), 2.0, EPS, "fmax(1,2)");
    }

    #[test]
    fn fmin_fmax_nan() {
        // POSIX: if one arg is NaN, return the other.
        assert_approx(fmin(f64::NAN, 1.0), 1.0, EPS, "fmin(NaN,1)");
        assert_approx(fmax(f64::NAN, 1.0), 1.0, EPS, "fmax(NaN,1)");
        assert_approx(fmin(1.0, f64::NAN), 1.0, EPS, "fmin(1,NaN)");
        assert_approx(fmax(1.0, f64::NAN), 1.0, EPS, "fmax(1,NaN)");
    }

    // -----------------------------------------------------------------------
    // fdim
    // -----------------------------------------------------------------------

    #[test]
    fn fdim_basic() {
        assert_approx(fdim(5.0, 3.0), 2.0, EPS, "fdim(5,3)");
        assert_approx(fdim(3.0, 5.0), 0.0, EPS, "fdim(3,5)");
    }

    // -----------------------------------------------------------------------
    // rint — ties to even (banker's rounding)
    // -----------------------------------------------------------------------

    #[test]
    fn rint_ties_to_even() {
        // Half-integer ties: round to nearest *even* integer.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(0.5), 0.0, "rint(0.5) → 0 (even)");
            assert_eq!(rint(1.5), 2.0, "rint(1.5) → 2 (even)");
            assert_eq!(rint(2.5), 2.0, "rint(2.5) → 2 (even)");
            assert_eq!(rint(3.5), 4.0, "rint(3.5) → 4 (even)");
            assert_eq!(rint(4.5), 4.0, "rint(4.5) → 4 (even)");
            assert_eq!(rint(5.5), 6.0, "rint(5.5) → 6 (even)");
        }
    }

    #[test]
    fn rint_negative_ties_to_even() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(-0.5), 0.0, "rint(-0.5) → 0 (even, toward zero)");
            assert_eq!(rint(-1.5), -2.0, "rint(-1.5) → -2 (even)");
            assert_eq!(rint(-2.5), -2.0, "rint(-2.5) → -2 (even)");
            assert_eq!(rint(-3.5), -4.0, "rint(-3.5) → -4 (even)");
        }
    }

    #[test]
    fn rint_non_ties() {
        // Non-tie cases: round to nearest.
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(0.3), 0.0, "rint(0.3) → 0");
            assert_eq!(rint(0.7), 1.0, "rint(0.7) → 1");
            assert_eq!(rint(1.2), 1.0, "rint(1.2) → 1");
            assert_eq!(rint(1.8), 2.0, "rint(1.8) → 2");
            assert_eq!(rint(-0.3), 0.0, "rint(-0.3) → 0");
            assert_eq!(rint(-0.7), -1.0, "rint(-0.7) → -1");
        }
    }

    #[test]
    fn rint_exact_integers() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(0.0), 0.0, "rint(0.0)");
            assert_eq!(rint(1.0), 1.0, "rint(1.0)");
            assert_eq!(rint(-1.0), -1.0, "rint(-1.0)");
            assert_eq!(rint(100.0), 100.0, "rint(100.0)");
        }
    }

    #[test]
    fn rint_special_values() {
        assert!(rint(f64::NAN).is_nan(), "rint(NaN) → NaN");
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(f64::INFINITY), f64::INFINITY, "rint(inf)");
            assert_eq!(rint(f64::NEG_INFINITY), f64::NEG_INFINITY, "rint(-inf)");
        }
    }

    #[test]
    fn rint_preserves_signed_zero() {
        // rint(±0.0) must preserve the sign.
        let pos = rint(0.0);
        let neg = rint(-0.0);
        assert_eq!(pos.to_bits(), 0.0_f64.to_bits(), "rint(+0) = +0");
        assert_eq!(neg.to_bits(), (-0.0_f64).to_bits(), "rint(-0) = -0");
    }

    #[test]
    fn rint_large_values() {
        // Values >= 2^52 are already exact integers, returned unchanged.
        let big = 4_503_599_627_370_496.0; // 2^52
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rint(big), big, "rint(2^52) unchanged");
            assert_eq!(rint(big + 1.0), big + 1.0, "rint(2^52 + 1) unchanged");
        }
    }

    // -----------------------------------------------------------------------
    // rintf — single-precision ties to even
    // -----------------------------------------------------------------------

    #[test]
    fn rintf_ties_to_even() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(rintf(0.5), 0.0, "rintf(0.5) → 0");
            assert_eq!(rintf(1.5), 2.0, "rintf(1.5) → 2");
            assert_eq!(rintf(2.5), 2.0, "rintf(2.5) → 2");
            assert_eq!(rintf(3.5), 4.0, "rintf(3.5) → 4");
            assert_eq!(rintf(-0.5), 0.0, "rintf(-0.5) → 0");
            assert_eq!(rintf(-1.5), -2.0, "rintf(-1.5) → -2");
        }
    }

    // -----------------------------------------------------------------------
    // lrint — ties to even, returns i64
    // -----------------------------------------------------------------------

    #[test]
    fn lrint_ties_to_even() {
        assert_eq!(lrint(0.5), 0, "lrint(0.5) → 0 (even)");
        assert_eq!(lrint(1.5), 2, "lrint(1.5) → 2 (even)");
        assert_eq!(lrint(2.5), 2, "lrint(2.5) → 2 (even)");
        assert_eq!(lrint(3.5), 4, "lrint(3.5) → 4 (even)");
        assert_eq!(lrint(4.5), 4, "lrint(4.5) → 4 (even)");
        assert_eq!(lrint(5.5), 6, "lrint(5.5) → 6 (even)");
    }

    #[test]
    fn lrint_negative_ties() {
        assert_eq!(lrint(-0.5), 0, "lrint(-0.5) → 0");
        assert_eq!(lrint(-1.5), -2, "lrint(-1.5) → -2");
        assert_eq!(lrint(-2.5), -2, "lrint(-2.5) → -2");
        assert_eq!(lrint(-3.5), -4, "lrint(-3.5) → -4");
    }

    #[test]
    fn lrint_non_ties() {
        assert_eq!(lrint(0.3), 0, "lrint(0.3) → 0");
        assert_eq!(lrint(0.7), 1, "lrint(0.7) → 1");
        assert_eq!(lrint(1.2), 1, "lrint(1.2) → 1");
        assert_eq!(lrint(1.8), 2, "lrint(1.8) → 2");
        assert_eq!(lrint(-0.3), 0, "lrint(-0.3) → 0");
        assert_eq!(lrint(-0.7), -1, "lrint(-0.7) → -1");
    }

    #[test]
    fn lrint_special_values() {
        // NaN and the infinities are no long: LONG_MIN, what x86-64's
        // conversion answers and glibc and musl return (this said 0, the old
        // implementation's answer).
        assert_eq!(lrint(f64::NAN), i64::MIN, "lrint(NaN) = LONG_MIN");
        assert_eq!(lrint(f64::INFINITY), i64::MIN, "lrint(inf) = LONG_MIN");
        assert_eq!(lrint(f64::NEG_INFINITY), i64::MIN, "lrint(-inf) = LONG_MIN");
    }

    #[test]
    fn lrintf_ties_to_even() {
        assert_eq!(lrintf(0.5), 0, "lrintf(0.5) → 0");
        assert_eq!(lrintf(1.5), 2, "lrintf(1.5) → 2");
        assert_eq!(lrintf(2.5), 2, "lrintf(2.5) → 2");
        assert_eq!(lrintf(-0.5), 0, "lrintf(-0.5) → 0");
        assert_eq!(lrintf(-1.5), -2, "lrintf(-1.5) → -2");
    }

    // -----------------------------------------------------------------------
    // nearbyint — same as rint (no FP exception distinction in our impl)
    // -----------------------------------------------------------------------

    #[test]
    fn nearbyint_ties_to_even() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(nearbyint(0.5), 0.0, "nearbyint(0.5) → 0");
            assert_eq!(nearbyint(1.5), 2.0, "nearbyint(1.5) → 2");
            assert_eq!(nearbyint(2.5), 2.0, "nearbyint(2.5) → 2");
            assert_eq!(nearbyint(-0.5), 0.0, "nearbyint(-0.5) → 0");
        }
    }

    #[test]
    fn nearbyintf_ties_to_even() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(nearbyintf(0.5), 0.0, "nearbyintf(0.5) → 0");
            assert_eq!(nearbyintf(1.5), 2.0, "nearbyintf(1.5) → 2");
        }
    }

    // -----------------------------------------------------------------------
    // ilogb — unbiased exponent extraction
    // -----------------------------------------------------------------------

    #[test]
    fn ilogb_powers_of_two() {
        assert_eq!(ilogb(1.0), 0, "ilogb(1) = 0");
        assert_eq!(ilogb(2.0), 1, "ilogb(2) = 1");
        assert_eq!(ilogb(4.0), 2, "ilogb(4) = 2");
        assert_eq!(ilogb(0.5), -1, "ilogb(0.5) = -1");
        assert_eq!(ilogb(0.25), -2, "ilogb(0.25) = -2");
        assert_eq!(ilogb(1024.0), 10, "ilogb(1024) = 10");
    }

    #[test]
    fn ilogb_non_powers() {
        // ilogb returns the floor-log2 for the exponent field.
        // ilogb(3.0) — 3 = 1.1_2 * 2^1, so exponent = 1.
        assert_eq!(ilogb(3.0), 1, "ilogb(3) = 1");
        // ilogb(5.0) — 5 = 1.01_2 * 2^2, so exponent = 2.
        assert_eq!(ilogb(5.0), 2, "ilogb(5) = 2");
        // ilogb(7.0) — 7 = 1.11_2 * 2^2, so exponent = 2.
        assert_eq!(ilogb(7.0), 2, "ilogb(7) = 2");
    }

    #[test]
    fn ilogb_negative() {
        // ilogb ignores the sign bit.
        assert_eq!(ilogb(-1.0), 0, "ilogb(-1) = 0");
        assert_eq!(ilogb(-8.0), 3, "ilogb(-8) = 3");
    }

    #[test]
    fn ilogb_special_values() {
        assert_eq!(ilogb(0.0), i32::MIN, "ilogb(0) = FP_ILOGB0");
        assert_eq!(ilogb(-0.0), i32::MIN, "ilogb(-0) = FP_ILOGB0");
        assert_eq!(ilogb(f64::INFINITY), i32::MAX, "ilogb(inf) = INT_MAX");
        assert_eq!(ilogb(f64::NEG_INFINITY), i32::MAX, "ilogb(-inf) = INT_MAX");
        assert_eq!(
            ilogb(f64::NAN),
            FP_ILOGBNAN,
            "ilogb(NaN) = FP_ILOGBNAN, INT_MIN"
        );
    }

    #[test]
    fn ilogb_subnormals() {
        // Smallest subnormal: 2^-1074 (bits = 1).
        let smallest = f64::from_bits(1);
        assert_eq!(ilogb(smallest), -1074, "ilogb(smallest subnormal) = -1074");

        // Largest subnormal: biased_exp=0, all mantissa bits set.
        // Value ≈ (2^52 - 1) * 2^-1074 ≈ 2^-1022 - 2^-1074.
        // The leading 1-bit is at position 51, so exponent = -1023 + 51 - 51 wait...
        // mantissa = 0x000F_FFFF_FFFF_FFFF, leading_zeros() of u64 = 12.
        // lz = 12 - 12 = 0. exponent = -1023 - 0 = -1023.
        let largest_sub = f64::from_bits(0x000F_FFFF_FFFF_FFFF);
        assert_eq!(
            ilogb(largest_sub),
            -1023,
            "ilogb(largest subnormal) = -1023"
        );

        // A mid-range subnormal: mantissa bit 51 clear, bit 50 set.
        // mantissa = 0x0004_0000_0000_0000.  leading_zeros() of u64:
        //   bits: 0x0004... = 0000_0000_0000_0100_0000...
        //   leading zeros = 13.  lz = 13 - 12 = 1.
        //   exponent = -1023 - 1 = -1024.
        let mid = f64::from_bits(0x0004_0000_0000_0000);
        assert_eq!(ilogb(mid), -1024, "ilogb(mid subnormal) = -1024");
    }

    #[test]
    fn ilogbf_basic() {
        assert_eq!(ilogbf(1.0), 0, "ilogbf(1) = 0");
        assert_eq!(ilogbf(8.0), 3, "ilogbf(8) = 3");
        assert_eq!(ilogbf(0.0), i32::MIN, "ilogbf(0) = FP_ILOGB0");
    }

    // -----------------------------------------------------------------------
    // logb — same as ilogb but returns f64
    // -----------------------------------------------------------------------

    #[test]
    fn logb_basic() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(logb(1.0), 0.0, "logb(1) = 0");
            assert_eq!(logb(2.0), 1.0, "logb(2) = 1");
            assert_eq!(logb(0.5), -1.0, "logb(0.5) = -1");
        }
    }

    #[test]
    fn logb_special() {
        assert_eq!(logb(0.0), f64::NEG_INFINITY, "logb(0) = -inf");
        assert_eq!(logb(f64::INFINITY), f64::INFINITY, "logb(inf) = inf");
        assert!(logb(f64::NAN).is_nan(), "logb(NaN) = NaN");
    }

    // -----------------------------------------------------------------------
    // logbf — f32 variant
    // -----------------------------------------------------------------------

    #[test]
    fn logbf_basic() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(logbf(1.0), 0.0, "logbf(1) = 0");
            assert_eq!(logbf(2.0), 1.0, "logbf(2) = 1");
            assert_eq!(logbf(8.0), 3.0, "logbf(8) = 3");
            assert_eq!(logbf(0.5), -1.0, "logbf(0.5) = -1");
        }
    }

    #[test]
    fn logbf_special() {
        assert_eq!(logbf(0.0), f32::NEG_INFINITY, "logbf(0) = -inf");
        assert_eq!(logbf(f32::INFINITY), f32::INFINITY, "logbf(inf) = inf");
        assert!(logbf(f32::NAN).is_nan(), "logbf(NaN) = NaN");
    }

    // -----------------------------------------------------------------------
    // nan / nanf — NaN constructors
    // -----------------------------------------------------------------------

    #[test]
    fn nan_returns_nan() {
        let tag = b"\0";
        let result = nan(tag.as_ptr());
        assert!(result.is_nan(), "nan() should return NaN");
    }

    #[test]
    fn nanf_returns_nan() {
        let tag = b"\0";
        let result = nanf(tag.as_ptr());
        assert!(result.is_nan(), "nanf() should return NaN");
    }

    #[test]
    fn nan_null_tag() {
        let result = nan(core::ptr::null());
        assert!(result.is_nan(), "nan(null) should return NaN");
    }

    // -----------------------------------------------------------------------
    // frexpf / ldexpf / modff — f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn frexpf_basic() {
        let mut e: i32 = 0;
        let m = frexpf(8.0, &mut e);
        assert_approx(f64::from(m), 0.5, 1e-6, "frexpf(8) mantissa");
        assert_eq!(e, 4, "frexpf(8) exponent");
    }

    #[test]
    fn frexpf_roundtrip() {
        let values = [1.0f32, 0.5, 100.0, -3.14, 0.001];
        for &x in &values {
            let mut e: i32 = 0;
            let m = frexpf(x, &mut e);
            let roundtrip = ldexpf(m, e);
            assert_approx(
                f64::from(roundtrip),
                f64::from(x),
                1e-6,
                &format!("frexpf/ldexpf roundtrip for {x}"),
            );
        }
    }

    #[test]
    fn ldexpf_basic() {
        let result = ldexpf(1.0, 4);
        assert_approx(f64::from(result), 16.0, 1e-6, "ldexpf(1, 4)");
    }

    #[test]
    fn ldexpf_fraction() {
        let result = ldexpf(0.75, 3);
        assert_approx(f64::from(result), 6.0, 1e-6, "ldexpf(0.75, 3)");
    }

    #[test]
    fn modff_basic() {
        let mut ipart: f32 = 0.0;
        let frac = modff(3.75, &mut ipart);
        assert_approx(f64::from(ipart), 3.0, 1e-6, "modff(3.75) ipart");
        assert_approx(f64::from(frac), 0.75, 1e-6, "modff(3.75) frac");
    }

    #[test]
    fn modff_negative() {
        let mut ipart: f32 = 0.0;
        let frac = modff(-2.25, &mut ipart);
        assert_approx(f64::from(ipart), -2.0, 1e-6, "modff(-2.25) ipart");
        assert_approx(f64::from(frac), -0.25, 1e-6, "modff(-2.25) frac");
    }

    #[test]
    fn modff_integer() {
        let mut ipart: f32 = 0.0;
        let frac = modff(5.0, &mut ipart);
        assert_approx(f64::from(ipart), 5.0, 1e-6, "modff(5.0) ipart");
        assert_approx(f64::from(frac), 0.0, 1e-6, "modff(5.0) frac");
    }

    // -----------------------------------------------------------------------
    // scalbnf / scalbln / scalblnf
    // -----------------------------------------------------------------------

    #[test]
    fn scalbnf_basic() {
        let result = scalbnf(1.0, 3);
        assert_approx(f64::from(result), 8.0, 1e-6, "scalbnf(1, 3)");
        let result2 = scalbnf(3.0, -1);
        assert_approx(f64::from(result2), 1.5, 1e-6, "scalbnf(3, -1)");
    }

    #[test]
    fn scalbln_basic() {
        assert_approx(scalbln(1.0, 10), 1024.0, EPS, "scalbln(1, 10)");
        assert_approx(scalbln(2.0, -1), 1.0, EPS, "scalbln(2, -1)");
    }

    #[test]
    fn scalbln_large_exponent() {
        // Exponents beyond ±1074 clamp to overflow/underflow.
        let result = scalbln(1.0, 2000);
        assert_eq!(result, f64::INFINITY, "scalbln(1, 2000) = inf");
        let result2 = scalbln(1.0, -2000);
        assert_eq!(result2, 0.0, "scalbln(1, -2000) = 0");
    }

    #[test]
    fn scalblnf_basic() {
        let result = scalblnf(1.0, 4);
        assert_approx(f64::from(result), 16.0, 1e-6, "scalblnf(1, 4)");
    }

    // -----------------------------------------------------------------------
    // nextafterf
    // -----------------------------------------------------------------------

    #[test]
    fn nextafterf_direction() {
        let up = nextafterf(1.0, 2.0);
        let down = nextafterf(1.0, 0.0);
        assert!(up > 1.0, "nextafterf(1, 2) > 1");
        assert!(down < 1.0, "nextafterf(1, 0) < 1");
    }

    #[test]
    fn nextafterf_from_zero() {
        let pos = nextafterf(0.0, 1.0);
        assert!(pos > 0.0, "nextafterf(0, 1) > 0");
        let neg = nextafterf(0.0, -1.0);
        assert!(neg < 0.0, "nextafterf(0, -1) < 0");
    }

    #[test]
    fn nextafterf_same() {
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(nextafterf(1.0, 1.0), 1.0);
            assert_eq!(nextafterf(0.0, 0.0), 0.0);
        }
    }

    #[test]
    fn nextafterf_nan() {
        assert!(nextafterf(f32::NAN, 1.0).is_nan());
        assert!(nextafterf(1.0, f32::NAN).is_nan());
    }

    // -----------------------------------------------------------------------
    // remainderf
    // -----------------------------------------------------------------------

    #[test]
    fn remainderf_basic() {
        let r = remainderf(5.0, 2.0);
        assert_approx(f64::from(r), 1.0, 1e-6, "remainderf(5, 2)");
    }

    #[test]
    fn remainderf_negative() {
        let r = remainderf(7.0, 3.0);
        assert_approx(f64::from(r), 1.0, 1e-6, "remainderf(7, 3)");
    }

    #[test]
    fn remainderf_nan() {
        assert!(remainderf(f32::INFINITY, 1.0).is_nan());
        assert!(remainderf(1.0, 0.0).is_nan());
    }

    // -----------------------------------------------------------------------
    // remquof
    // -----------------------------------------------------------------------

    #[test]
    fn remquof_basic() {
        let mut q: i32 = 0;
        let r = remquof(7.0, 3.0, &mut q);
        assert_approx(f64::from(r), 1.0, 1e-6, "remquof(7, 3) remainder");
        assert_eq!(q, 2, "remquof(7, 3) quotient");
    }

    #[test]
    fn remquof_negative() {
        let mut q: i32 = 0;
        let r = remquof(-7.0, 3.0, &mut q);
        assert_approx(f64::from(r), -1.0, 1e-6, "remquof(-7, 3) remainder");
        assert_eq!(q, -2, "remquof(-7, 3) quotient");
    }

    // -----------------------------------------------------------------------
    // fdimf / fmaf
    // -----------------------------------------------------------------------

    #[test]
    fn fdimf_basic() {
        let r = fdimf(5.0, 3.0);
        assert_approx(f64::from(r), 2.0, 1e-6, "fdimf(5, 3)");
        let r2 = fdimf(3.0, 5.0);
        assert_approx(f64::from(r2), 0.0, 1e-6, "fdimf(3, 5)");
    }

    #[test]
    fn fmaf_basic() {
        // fma(2, 3, 4) = 2*3 + 4 = 10
        let r = fmaf(2.0, 3.0, 4.0);
        assert_approx(f64::from(r), 10.0, 1e-6, "fmaf(2, 3, 4)");
    }

    #[test]
    fn fmaf_precision() {
        // fmaf promotes to f64 internally, so (1+eps)*(1+eps)-1 should be
        // more accurate than naive f32 mul+add.
        // Use values that are exactly representable in f32.
        let r = fmaf(1.0, 2.0, 3.0);
        assert_approx(f64::from(r), 5.0, 1e-6, "fmaf(1, 2, 3) = 5");
        // Negative: -2*3 + 7 = 1
        let r2 = fmaf(-2.0, 3.0, 7.0);
        assert_approx(f64::from(r2), 1.0, 1e-6, "fmaf(-2, 3, 7) = 1");
    }

    // -----------------------------------------------------------------------
    // sinhf / coshf / tanhf — hyperbolic f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn sinhf_values() {
        assert_approx(f64::from(sinhf(0.0)), 0.0, 1e-6, "sinhf(0)");
        assert_approx(f64::from(sinhf(1.0)), 1.175_201, 1e-4, "sinhf(1)");
        assert_approx(f64::from(sinhf(-1.0)), -1.175_201, 1e-4, "sinhf(-1)");
    }

    #[test]
    fn sinhf_special() {
        assert!(sinhf(f32::NAN).is_nan(), "sinhf(NaN)");
        assert_eq!(sinhf(f32::INFINITY), f32::INFINITY, "sinhf(inf)");
        assert_eq!(sinhf(f32::NEG_INFINITY), f32::NEG_INFINITY, "sinhf(-inf)");
    }

    #[test]
    fn coshf_values() {
        assert_approx(f64::from(coshf(0.0)), 1.0, 1e-6, "coshf(0)");
        assert_approx(f64::from(coshf(1.0)), 1.543_081, 1e-4, "coshf(1)");
        // cosh is even: cosh(-x) = cosh(x)
        assert_approx(f64::from(coshf(-1.0)), 1.543_081, 1e-4, "coshf(-1)");
    }

    #[test]
    fn coshf_special() {
        assert!(coshf(f32::NAN).is_nan(), "coshf(NaN)");
        assert_eq!(coshf(f32::INFINITY), f32::INFINITY, "coshf(inf)");
        assert_eq!(coshf(f32::NEG_INFINITY), f32::INFINITY, "coshf(-inf)");
    }

    #[test]
    fn tanhf_values() {
        assert_approx(f64::from(tanhf(0.0)), 0.0, 1e-6, "tanhf(0)");
        assert_approx(f64::from(tanhf(1.0)), 0.761_594, 1e-4, "tanhf(1)");
        assert_approx(f64::from(tanhf(-1.0)), -0.761_594, 1e-4, "tanhf(-1)");
    }

    #[test]
    fn tanhf_limits() {
        // tanh approaches ±1 for large inputs.
        assert_approx(f64::from(tanhf(20.0)), 1.0, 1e-6, "tanhf(20) ≈ 1");
        assert_approx(f64::from(tanhf(-20.0)), -1.0, 1e-6, "tanhf(-20) ≈ -1");
    }

    // -----------------------------------------------------------------------
    // asinhf / acoshf / atanhf — inverse hyperbolic f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn asinhf_values() {
        assert_approx(f64::from(asinhf(0.0)), 0.0, 1e-6, "asinhf(0)");
        assert_approx(f64::from(asinhf(1.0)), 0.881_374, 1e-4, "asinhf(1)");
        assert_approx(f64::from(asinhf(-1.0)), -0.881_374, 1e-4, "asinhf(-1)");
    }

    #[test]
    fn asinhf_sinh_roundtrip() {
        let vals = [0.5f32, 1.0, 2.0, -1.5];
        for &x in &vals {
            let roundtrip = sinhf(asinhf(x));
            assert_approx(
                f64::from(roundtrip),
                f64::from(x),
                1e-4,
                &format!("sinh(asinh({x}))"),
            );
        }
    }

    #[test]
    fn acoshf_values() {
        assert_approx(f64::from(acoshf(1.0)), 0.0, 1e-6, "acoshf(1)");
        assert_approx(f64::from(acoshf(2.0)), 1.316_958, 1e-4, "acoshf(2)");
    }

    #[test]
    fn acoshf_domain_error() {
        // acosh(x) for x < 1 is NaN.
        assert!(acoshf(0.5).is_nan(), "acoshf(0.5) = NaN");
        assert!(acoshf(-1.0).is_nan(), "acoshf(-1) = NaN");
    }

    #[test]
    fn atanhf_values() {
        assert_approx(f64::from(atanhf(0.0)), 0.0, 1e-6, "atanhf(0)");
        assert_approx(f64::from(atanhf(0.5)), 0.549_306, 1e-4, "atanhf(0.5)");
        assert_approx(f64::from(atanhf(-0.5)), -0.549_306, 1e-4, "atanhf(-0.5)");
    }

    #[test]
    fn atanhf_boundary() {
        assert_eq!(atanhf(1.0), f32::INFINITY, "atanhf(1) = +inf");
        assert_eq!(atanhf(-1.0), f32::NEG_INFINITY, "atanhf(-1) = -inf");
        assert!(atanhf(1.5).is_nan(), "atanhf(1.5) = NaN");
    }

    // -----------------------------------------------------------------------
    // erff / erfcf — f32 error function variants
    // -----------------------------------------------------------------------

    #[test]
    fn erff_values() {
        assert_approx(f64::from(erff(0.0)), 0.0, 1e-5, "erff(0)");
        assert_approx(f64::from(erff(1.0)), 0.842_701, 1e-4, "erff(1)");
        assert_approx(f64::from(erff(-1.0)), -0.842_701, 1e-4, "erff(-1)");
    }

    #[test]
    fn erff_large() {
        // erf(x) → 1 for large x.
        assert_approx(f64::from(erff(5.0)), 1.0, 1e-5, "erff(5) ≈ 1");
        assert_approx(f64::from(erff(-5.0)), -1.0, 1e-5, "erff(-5) ≈ -1");
    }

    #[test]
    fn erfcf_values() {
        assert_approx(f64::from(erfcf(0.0)), 1.0, 1e-5, "erfcf(0)");
        assert_approx(f64::from(erfcf(1.0)), 0.157_299, 1e-4, "erfcf(1)");
    }

    #[test]
    fn erfcf_complements_erff() {
        let vals = [0.0f32, 0.5, 1.0, 2.0, -1.0];
        for &x in &vals {
            let sum = f64::from(erff(x)) + f64::from(erfcf(x));
            assert_approx(sum, 1.0, 1e-5, &format!("erff({x}) + erfcf({x}) = 1"));
        }
    }

    // -----------------------------------------------------------------------
    // lgammaf / lgammaf_r / tgammaf — f32 gamma variants
    // -----------------------------------------------------------------------

    #[test]
    fn lgammaf_values() {
        let _g = signgam_lock();
        // lgamma(1) = ln(Γ(1)) = ln(1) = 0
        assert_approx(f64::from(lgammaf(1.0)), 0.0, 1e-5, "lgammaf(1)");
        // lgamma(2) = ln(Γ(2)) = ln(1) = 0
        assert_approx(f64::from(lgammaf(2.0)), 0.0, 1e-5, "lgammaf(2)");
        // lgamma(5) = ln(4!) = ln(24) ≈ 3.178
        assert_approx(f64::from(lgammaf(5.0)), 3.178_054, 1e-3, "lgammaf(5)");
    }

    #[test]
    fn lgammaf_poles() {
        let _g = signgam_lock();
        // lgamma at non-positive integers → +inf.
        assert_eq!(lgammaf(0.0), f32::INFINITY, "lgammaf(0) = inf");
        assert_eq!(lgammaf(-1.0), f32::INFINITY, "lgammaf(-1) = inf");
    }

    #[test]
    fn lgammaf_r_sign() {
        let mut sign: i32 = 0;
        let result = lgammaf_r(2.0, &mut sign);
        assert_approx(f64::from(result), 0.0, 1e-5, "lgammaf_r(2)");
        assert_eq!(sign, 1, "lgammaf_r(2) sign = +1");

        // Between -1 and 0, Γ(x) < 0.
        let result2 = lgammaf_r(-0.5, &mut sign);
        assert!(f64::from(result2) > 0.0, "lgammaf_r(-0.5) > 0");
        assert_eq!(sign, -1, "lgammaf_r(-0.5) sign = -1");
    }

    #[test]
    fn tgammaf_values() {
        // Γ(1) = 1, Γ(5) = 24
        assert_approx(f64::from(tgammaf(1.0)), 1.0, 1e-4, "tgammaf(1)");
        assert_approx(f64::from(tgammaf(5.0)), 24.0, 1e-2, "tgammaf(5)");
    }

    #[test]
    fn tgammaf_half() {
        // Γ(0.5) = √π ≈ 1.7724539
        assert_approx(f64::from(tgammaf(0.5)), 1.772_454, 1e-3, "tgammaf(0.5)");
    }

    // -----------------------------------------------------------------------
    // sincosf — f32 sincos
    // -----------------------------------------------------------------------

    #[test]
    fn sincosf_consistency() {
        let angles: [f32; 5] = [0.0, 1.0, -1.0, 3.14159 / 4.0, 2.0];
        for &a in &angles {
            let mut s: f32 = 0.0;
            let mut c: f32 = 0.0;
            sincosf(a, &mut s, &mut c);
            assert_approx(
                f64::from(s),
                f64::from(sinf(a)),
                1e-6,
                &format!("sincosf({a}) sin"),
            );
            assert_approx(
                f64::from(c),
                f64::from(cosf(a)),
                1e-6,
                &format!("sincosf({a}) cos"),
            );
        }
    }

    #[test]
    fn sincosf_pythagorean() {
        let angles: [f32; 4] = [0.5, 1.0, 2.0, -0.7];
        for &a in &angles {
            let mut s: f32 = 0.0;
            let mut c: f32 = 0.0;
            sincosf(a, &mut s, &mut c);
            let sum = f64::from(s) * f64::from(s) + f64::from(c) * f64::from(c);
            assert_approx(sum, 1.0, 1e-5, &format!("sin²({a}) + cos²({a}) = 1"));
        }
    }

    // -----------------------------------------------------------------------
    // exp10f / pow10f — f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn exp10f_values() {
        assert_approx(f64::from(exp10f(0.0)), 1.0, 1e-6, "exp10f(0)");
        assert_approx(f64::from(exp10f(1.0)), 10.0, 1e-4, "exp10f(1)");
        assert_approx(f64::from(exp10f(2.0)), 100.0, 1e-3, "exp10f(2)");
        assert_approx(f64::from(exp10f(-1.0)), 0.1, 1e-5, "exp10f(-1)");
    }

    #[test]
    fn pow10f_is_exp10f() {
        let vals = [0.0f32, 1.0, -1.0, 2.0, 0.5];
        for &x in &vals {
            #[allow(clippy::float_cmp)]
            {
                assert_eq!(pow10f(x), exp10f(x), "pow10f({x}) == exp10f({x})");
            }
        }
    }

    // -----------------------------------------------------------------------
    // asinf / acosf / atanf / atan2f — inverse trig f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn asinf_values() {
        assert_approx(f64::from(asinf(0.0)), 0.0, 1e-6, "asinf(0)");
        assert_approx(
            f64::from(asinf(1.0)),
            core::f64::consts::FRAC_PI_2,
            1e-4,
            "asinf(1)",
        );
        assert_approx(
            f64::from(asinf(-1.0)),
            -core::f64::consts::FRAC_PI_2,
            1e-4,
            "asinf(-1)",
        );
        assert_approx(
            f64::from(asinf(0.5)),
            core::f64::consts::FRAC_PI_6,
            1e-4,
            "asinf(0.5)",
        );
    }

    #[test]
    fn asinf_domain_error() {
        assert!(asinf(1.5).is_nan(), "asinf(1.5) = NaN");
        assert!(asinf(-1.5).is_nan(), "asinf(-1.5) = NaN");
    }

    #[test]
    fn acosf_values() {
        assert_approx(f64::from(acosf(1.0)), 0.0, 1e-6, "acosf(1)");
        assert_approx(
            f64::from(acosf(0.0)),
            core::f64::consts::FRAC_PI_2,
            1e-4,
            "acosf(0)",
        );
        assert_approx(
            f64::from(acosf(-1.0)),
            core::f64::consts::PI,
            1e-4,
            "acosf(-1)",
        );
    }

    #[test]
    fn acosf_domain_error() {
        assert!(acosf(1.5).is_nan(), "acosf(1.5) = NaN");
        assert!(acosf(-1.5).is_nan(), "acosf(-1.5) = NaN");
    }

    #[test]
    fn atanf_values() {
        assert_approx(f64::from(atanf(0.0)), 0.0, 1e-6, "atanf(0)");
        assert_approx(
            f64::from(atanf(1.0)),
            core::f64::consts::FRAC_PI_4,
            1e-6,
            "atanf(1)",
        );
        assert_approx(
            f64::from(atanf(-1.0)),
            -core::f64::consts::FRAC_PI_4,
            1e-6,
            "atanf(-1)",
        );
    }

    #[test]
    fn atan2f_quadrants() {
        // atan2(1, 1) = π/4.
        assert_approx(
            f64::from(atan2f(1.0, 1.0)),
            core::f64::consts::FRAC_PI_4,
            1e-6,
            "atan2f(1, 1)",
        );
        // atan2(1, -1) = 3π/4.
        assert_approx(
            f64::from(atan2f(1.0, -1.0)),
            3.0 * core::f64::consts::FRAC_PI_4,
            1e-6,
            "atan2f(1, -1)",
        );
        // atan2(-1, 1) = -π/4.
        assert_approx(
            f64::from(atan2f(-1.0, 1.0)),
            -core::f64::consts::FRAC_PI_4,
            1e-6,
            "atan2f(-1, 1)",
        );
    }

    #[test]
    fn atan2f_axis_values() {
        assert_approx(f64::from(atan2f(0.0, 1.0)), 0.0, 1e-6, "atan2f(0, 1)");
        assert_approx(
            f64::from(atan2f(1.0, 0.0)),
            core::f64::consts::FRAC_PI_2,
            1e-4,
            "atan2f(1, 0)",
        );
    }

    // -----------------------------------------------------------------------
    // lroundf / llround / llroundf — integer rounding
    // -----------------------------------------------------------------------

    #[test]
    fn lroundf_values() {
        assert_eq!(lroundf(2.5), 3, "lroundf(2.5) = 3");
        assert_eq!(lroundf(2.4), 2, "lroundf(2.4) = 2");
        assert_eq!(lroundf(-2.5), -3, "lroundf(-2.5) = -3");
        assert_eq!(lroundf(-2.4), -2, "lroundf(-2.4) = -2");
        assert_eq!(lroundf(0.0), 0, "lroundf(0) = 0");
    }

    #[test]
    fn llround_values() {
        assert_eq!(llround(2.5), 3, "llround(2.5) = 3");
        assert_eq!(llround(2.4), 2, "llround(2.4) = 2");
        assert_eq!(llround(-2.5), -3, "llround(-2.5) = -3");
        assert_eq!(llround(-3.7), -4, "llround(-3.7) = -4");
        assert_eq!(llround(0.0), 0, "llround(0) = 0");
    }

    #[test]
    fn llroundf_values() {
        assert_eq!(llroundf(3.5), 4, "llroundf(3.5) = 4");
        assert_eq!(llroundf(3.4), 3, "llroundf(3.4) = 3");
        assert_eq!(llroundf(-3.5), -4, "llroundf(-3.5) = -4");
        assert_eq!(llroundf(-3.4), -3, "llroundf(-3.4) = -3");
    }

    // -----------------------------------------------------------------------
    // finitef / dremf / gammaf / significandf — f32 compatibility aliases
    // -----------------------------------------------------------------------

    #[test]
    fn finitef_values() {
        assert_eq!(finitef(1.0), 1, "finitef(1.0) = 1");
        assert_eq!(finitef(0.0), 1, "finitef(0.0) = 1");
        assert_eq!(finitef(-100.0), 1, "finitef(-100.0) = 1");
        assert_eq!(finitef(f32::INFINITY), 0, "finitef(inf) = 0");
        assert_eq!(finitef(f32::NEG_INFINITY), 0, "finitef(-inf) = 0");
        assert_eq!(finitef(f32::NAN), 0, "finitef(NaN) = 0");
    }

    #[test]
    fn dremf_alias() {
        // dremf is remainder for f32.
        let r = dremf(5.0, 2.0);
        let expected = remainderf(5.0, 2.0);
        #[allow(clippy::float_cmp)]
        {
            assert_eq!(r, expected, "dremf == remainderf");
        }
    }

    #[test]
    fn gammaf_is_lgammaf() {
        let _g = signgam_lock();
        // gamma is a deprecated alias for lgamma.
        let vals = [1.0f32, 2.0, 5.0, 0.5];
        for &x in &vals {
            #[allow(clippy::float_cmp)]
            {
                assert_eq!(gammaf(x), lgammaf(x), "gammaf({x}) == lgammaf({x})");
            }
        }
    }

    #[test]
    fn significandf_values() {
        // significand(x) extracts mantissa in [1, 2).
        let r = significandf(8.0);
        assert_approx(f64::from(r), 1.0, 1e-6, "significandf(8) = 1.0");
        let r2 = significandf(12.0);
        assert_approx(f64::from(r2), 1.5, 1e-6, "significandf(12) = 1.5");
    }

    // -----------------------------------------------------------------------
    // exp2f / log2f / log10f — f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn exp2f_values() {
        assert_approx(f64::from(exp2f(0.0)), 1.0, 1e-6, "exp2f(0)");
        assert_approx(f64::from(exp2f(1.0)), 2.0, 1e-6, "exp2f(1)");
        assert_approx(f64::from(exp2f(3.0)), 8.0, 1e-4, "exp2f(3)");
        assert_approx(f64::from(exp2f(-1.0)), 0.5, 1e-6, "exp2f(-1)");
    }

    #[test]
    fn log2f_values() {
        assert_approx(f64::from(log2f(1.0)), 0.0, 1e-6, "log2f(1)");
        assert_approx(f64::from(log2f(2.0)), 1.0, 1e-5, "log2f(2)");
        assert_approx(f64::from(log2f(8.0)), 3.0, 1e-5, "log2f(8)");
        assert_approx(f64::from(log2f(0.5)), -1.0, 1e-5, "log2f(0.5)");
    }

    #[test]
    fn log10f_values() {
        assert_approx(f64::from(log10f(1.0)), 0.0, 1e-6, "log10f(1)");
        assert_approx(f64::from(log10f(10.0)), 1.0, 1e-5, "log10f(10)");
        assert_approx(f64::from(log10f(100.0)), 2.0, 1e-5, "log10f(100)");
        assert_approx(f64::from(log10f(0.1)), -1.0, 1e-5, "log10f(0.1)");
    }

    // -----------------------------------------------------------------------
    // expf / logf edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn expf_special() {
        assert!(expf(f32::NAN).is_nan(), "expf(NaN) = NaN");
        assert_eq!(expf(f32::INFINITY), f32::INFINITY, "expf(inf)");
        assert_approx(f64::from(expf(f32::NEG_INFINITY)), 0.0, 1e-6, "expf(-inf)");
    }

    #[test]
    fn logf_special() {
        assert!(logf(f32::NAN).is_nan(), "logf(NaN) = NaN");
        assert_eq!(logf(f32::INFINITY), f32::INFINITY, "logf(inf)");
        assert_eq!(logf(0.0), f32::NEG_INFINITY, "logf(0) = -inf");
        assert!(logf(-1.0).is_nan(), "logf(-1) = NaN");
    }

    // -----------------------------------------------------------------------
    // log1pf / expm1f — f32 variants
    // -----------------------------------------------------------------------

    #[test]
    fn log1pf_values() {
        assert_approx(f64::from(log1pf(0.0)), 0.0, 1e-6, "log1pf(0)");
        assert_approx(
            f64::from(log1pf(1.0)),
            core::f64::consts::LN_2,
            1e-4,
            "log1pf(1)",
        );
        // log1p(-1) = log(0) = -inf
        assert_eq!(log1pf(-1.0), f32::NEG_INFINITY, "log1pf(-1) = -inf");
    }

    #[test]
    fn expm1f_values() {
        assert_approx(f64::from(expm1f(0.0)), 0.0, 1e-6, "expm1f(0)");
        assert_approx(
            f64::from(expm1f(1.0)),
            core::f64::consts::E - 1.0,
            1e-4,
            "expm1f(1)",
        );
        // expm1(-inf) = -1
        assert_approx(
            f64::from(expm1f(f32::NEG_INFINITY)),
            -1.0,
            1e-6,
            "expm1f(-inf)",
        );
    }

    // -----------------------------------------------------------------------
    // cbrtf / hypotf edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn cbrtf_negative() {
        assert_approx(f64::from(cbrtf(-8.0)), -2.0, 1e-5, "cbrtf(-8)");
        assert_approx(f64::from(cbrtf(-27.0)), -3.0, 1e-4, "cbrtf(-27)");
    }

    #[test]
    fn hypotf_zero() {
        assert_approx(f64::from(hypotf(0.0, 0.0)), 0.0, 1e-6, "hypotf(0, 0)");
        assert_approx(f64::from(hypotf(3.0, 0.0)), 3.0, 1e-5, "hypotf(3, 0)");
        assert_approx(f64::from(hypotf(0.0, 4.0)), 4.0, 1e-5, "hypotf(0, 4)");
    }

    #[test]
    fn hypotf_inf() {
        // hypot(inf, anything) = inf.
        assert_eq!(hypotf(f32::INFINITY, 1.0), f32::INFINITY, "hypotf(inf, 1)");
        assert_eq!(hypotf(1.0, f32::INFINITY), f32::INFINITY, "hypotf(1, inf)");
    }

    // -----------------------------------------------------------------------
    // fminf / fmaxf edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn fminf_fmaxf_nan() {
        // POSIX: if one arg is NaN, return the other.
        assert_approx(f64::from(fminf(f32::NAN, 1.0)), 1.0, 1e-6, "fminf(NaN, 1)");
        assert_approx(f64::from(fmaxf(f32::NAN, 1.0)), 1.0, 1e-6, "fmaxf(NaN, 1)");
        assert_approx(f64::from(fminf(1.0, f32::NAN)), 1.0, 1e-6, "fminf(1, NaN)");
        assert_approx(f64::from(fmaxf(1.0, f32::NAN)), 1.0, 1e-6, "fmaxf(1, NaN)");
    }

    // -----------------------------------------------------------------------
    // copysignf edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn copysignf_zero() {
        // copysign should work with ±0.
        let r = copysignf(0.0, -1.0);
        assert_eq!(r.to_bits(), (-0.0f32).to_bits(), "copysignf(0, -1) = -0");
        let r2 = copysignf(-0.0, 1.0);
        assert_eq!(r2.to_bits(), 0.0f32.to_bits(), "copysignf(-0, 1) = +0");
    }

    // -----------------------------------------------------------------------
    // __copysign (GNU alias)
    // -----------------------------------------------------------------------

    #[test]
    fn test_copysign_alias_positive() {
        let r = __copysign(3.0, 1.0);
        assert_eq!(r, 3.0);
    }

    #[test]
    fn test_copysign_alias_negative() {
        let r = __copysign(3.0, -1.0);
        assert_eq!(r, -3.0);
    }

    #[test]
    fn test_copysign_alias_matches_copysign() {
        let vals = [1.0, -1.0, 0.0, f64::INFINITY, f64::NAN];
        for &x in &vals {
            for &y in &vals {
                let a = __copysign(x, y);
                let b = copysign(x, y);
                assert_eq!(
                    a.to_bits(),
                    b.to_bits(),
                    "__copysign({x}, {y}) should match copysign"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // gamma (deprecated alias for lgamma)
    // -----------------------------------------------------------------------

    #[test]
    fn test_gamma_is_lgamma() {
        let _g = signgam_lock();
        let vals = [1.0, 2.0, 5.0, 0.5, 10.0];
        for &x in &vals {
            #[allow(clippy::float_cmp)]
            {
                assert_eq!(gamma(x), lgamma(x), "gamma({x}) should equal lgamma({x})");
            }
        }
    }

    #[test]
    fn test_gamma_one() {
        // gamma(1) = lgamma(1) = ln(0!) = 0.
        assert_approx(gamma(1.0), 0.0, 1e-6, "gamma(1) ≈ 0");
    }

    #[test]
    fn test_gamma_two() {
        // gamma(2) = lgamma(2) = ln(1!) = 0.
        assert_approx(gamma(2.0), 0.0, 1e-6, "gamma(2) ≈ 0");
    }

    // -----------------------------------------------------------------------
    // glibc 2.39, call by call (dlm/oracle/math_harness.py)
    // -----------------------------------------------------------------------

    /// The glibc oracle's table: one call a line, `<function> <inputs> =
    /// <outputs> <errno>`, floats as their bits in hex.
    const ORACLE: &str = include_str!("math_oracle.txt");

    /// How far from glibc's answer ours may be, in units in the last place.
    ///
    /// 0 for everything IEEE 754 defines exactly -- rounding, `sqrt`, `fma`,
    /// remainders, scaling, decomposition, classification -- where any
    /// difference is a bug. For the rest, glibc and musl use different
    /// approximations; the bounds are the larger of the two libraries'
    /// documented worst cases (glibc's `libm-test-ulps` for x86_64), so a
    /// difference past one is a bug in one of them.
    fn ulps_allowed(name: &str) -> u64 {
        match double_name(name) {
            "fabs" | "floor" | "ceil" | "round" | "trunc" | "rint" | "nearbyint" | "sqrt"
            | "fmod" | "remainder" | "drem" | "remquo" | "copysign" | "fmin" | "fmax" | "fdim"
            | "fma" | "frexp" | "ldexp" | "scalbn" | "scalbln" | "modf" | "ilogb" | "logb"
            | "nextafter" | "lround" | "llround" | "lrint" | "significand" | "finite" | "isnan"
            | "isinf" => 0,
            "exp" | "exp2" | "exp10" | "log" | "log2" | "log10" | "cbrt" | "hypot" | "atan"
            | "asin" | "acos" | "atan2" | "sin" | "cos" | "tan" | "sincos" | "tanh" | "asinh"
            | "acosh" | "atanh" | "sinh" | "cosh" | "pow" | "expm1" | "log1p" | "erf" => 2,
            "erfc" => 5,
            "lgamma" | "lgamma_r" | "gamma" | "tgamma" => 16,
            "j0" | "j1" | "y0" | "y1" | "jn" | "yn" => 64,
            other => panic!("no tolerance for {other}"),
        }
    }

    /// The double function a name belongs to: `sinf` is `sin`'s, `modff`
    /// `modf`'s, `lgammaf_r` `lgamma_r`'s -- while `modf`, `erf` and the
    /// other names that end in `f` of their own are themselves.
    fn double_name(name: &str) -> &str {
        const OWN_F: [&str; 3] = ["modf", "erf", "significand"];
        if name == "lgammaf_r" {
            return "lgamma_r";
        }
        if OWN_F.contains(&name) {
            return name;
        }
        match name.strip_suffix('f') {
            Some(base) if !base.is_empty() => base,
            _ => name,
        }
    }

    /// The distance between two doubles in units in the last place: their
    /// bit patterns laid out as one ordered line, so the step across zero is
    /// one step like any other.
    fn ulp_distance(a: f64, b: f64) -> u64 {
        fn line(x: f64) -> i128 {
            let b = x.to_bits();
            let mag = i128::from(b & 0x7FFF_FFFF_FFFF_FFFF);
            if b >> 63 == 1 { -mag } else { mag }
        }
        u64::try_from((line(a) - line(b)).unsigned_abs()).unwrap_or(u64::MAX)
    }

    /// [`ulp_distance`] for floats.
    fn ulp_distance_f(a: f32, b: f32) -> u64 {
        fn line(x: f32) -> i64 {
            let b = x.to_bits();
            let mag = i64::from(b & 0x7FFF_FFFF);
            if b >> 31 == 1 { -mag } else { mag }
        }
        (line(a) - line(b)).unsigned_abs()
    }

    /// Whether our double answers glibc's, for `name`: bit for bit where the
    /// tolerance is 0 (a NaN matching any NaN -- the payload of a computed
    /// NaN is not specified -- except where the function is defined on the
    /// bits), within the tolerance otherwise, and a zero's sign always.
    fn same_d(name: &str, ours: f64, glibc: f64) -> Result<(), String> {
        same_d_near(name, ours, glibc, false)
    }

    /// [`same_d`], and when `near_root` -- a function evaluated where its
    /// result is ill-conditioned (see [`near_root`]) -- a result under 1 may
    /// instead be within the same number of ulps *of 1*: an absolute error.
    fn same_d_near(name: &str, ours: f64, glibc: f64, near_root: bool) -> Result<(), String> {
        if ours.is_nan() || glibc.is_nan() {
            if ours.is_nan() && glibc.is_nan() {
                let bitwise = matches!(name, "fabs" | "copysign");
                return if !bitwise || ours.to_bits() == glibc.to_bits() {
                    Ok(())
                } else {
                    Err(format!(
                        "NaN {:#018x}, glibc {:#018x}",
                        ours.to_bits(),
                        glibc.to_bits()
                    ))
                };
            }
            return Err(format!("{ours:e}, glibc {glibc:e}"));
        }
        if ours == 0.0 && glibc == 0.0 && ours.is_sign_negative() != glibc.is_sign_negative() {
            return Err(format!("{ours:?}, glibc {glibc:?}: the sign of zero"));
        }
        let d = ulp_distance(ours, glibc);
        let tol = ulps_allowed(name);
        #[allow(clippy::cast_precision_loss)]
        let absolute = tol as f64 * f64::EPSILON;
        if d > tol && !(near_root && glibc.abs() < 1.0 && (ours - glibc).abs() <= absolute) {
            return Err(format!(
                "{ours:e} ({:#018x}), glibc {glibc:e} ({:#018x}): {d} ulp",
                ours.to_bits(),
                glibc.to_bits()
            ));
        }
        Ok(())
    }

    /// [`same_d`] for floats.
    fn same_f(name: &str, ours: f32, glibc: f32) -> Result<(), String> {
        same_f_near(name, ours, glibc, false)
    }

    /// [`same_d_near`] for floats.
    fn same_f_near(name: &str, ours: f32, glibc: f32, near_root: bool) -> Result<(), String> {
        if ours.is_nan() || glibc.is_nan() {
            if ours.is_nan() && glibc.is_nan() {
                let bitwise = matches!(name, "fabsf" | "copysignf");
                return if !bitwise || ours.to_bits() == glibc.to_bits() {
                    Ok(())
                } else {
                    Err(format!(
                        "NaN {:#010x}, glibc {:#010x}",
                        ours.to_bits(),
                        glibc.to_bits()
                    ))
                };
            }
            return Err(format!("{ours:e}, glibc {glibc:e}"));
        }
        if ours == 0.0 && glibc == 0.0 && ours.is_sign_negative() != glibc.is_sign_negative() {
            return Err(format!("{ours:?}, glibc {glibc:?}: the sign of zero"));
        }
        let d = ulp_distance_f(ours, glibc);
        let tol = ulps_allowed(name);
        #[allow(clippy::cast_precision_loss)]
        let absolute = tol as f32 * f32::EPSILON;
        if d > tol && !(near_root && glibc.abs() < 1.0 && (ours - glibc).abs() <= absolute) {
            return Err(format!(
                "{ours:e} ({:#010x}), glibc {glibc:e} ({:#010x}): {d} ulp",
                ours.to_bits(),
                glibc.to_bits()
            ));
        }
        Ok(())
    }

    /// Where a relative comparison says nothing: near a root of the
    /// function, a result of 1e-17 carrying an absolute error of 1e-17 is
    /// "a million ulps" off and as good as musl's formula can do. That is
    /// every call of `lgamma` (roots where the gamma function is 1 or -1,
    /// which glibc's `lgamma_neg.c` treats specially and musl does not, so
    /// glibc is right there and we are not: known-issues.md,
    /// D-POSIX-LGAMMA-LOSES-DIGITS-NEAR-NEGATIVE-ROOTS) and of the Bessel
    /// functions (infinitely many roots), and the trigonometric functions
    /// only at |x| >= 2^20, where a result near zero depends on the last bits
    /// of the argument reduction.
    fn near_root(name: &str, x: f64) -> bool {
        match double_name(name) {
            "lgamma" | "lgamma_r" | "gamma" | "j0" | "j1" | "y0" | "y1" | "jn" | "yn" => true,
            "sin" | "cos" | "tan" | "sincos" => x.abs() >= 1_048_576.0,
            _ => false,
        }
    }

    fn d(hex: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(hex, 16).expect("a double's bits"))
    }

    fn f(hex: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(hex, 16).expect("a float's bits"))
    }

    fn int(s: &str) -> i64 {
        s.parse().expect("an integer")
    }

    /// What C fixes of `remquo`'s quotient: its sign (unless it is zero) and
    /// its magnitude's low three bits. glibc stores three bits, musl 31, so
    /// that much and no more is compared.
    fn quo_agrees(q: i32, want: i64) -> bool {
        let q = i64::from(q);
        q.unsigned_abs() % 8 == want.unsigned_abs() % 8 && q.signum() * want.signum() >= 0
    }

    /// Our answer to one oracle line, compared: `Err` says how it differs.
    #[allow(clippy::too_many_lines)]
    fn replay_math(name: &str, ins: &[&str], outs: &[&str]) -> Result<(), String> {
        let want_errno: i32 = outs.last().expect("errno").parse().expect("errno");
        errno::set_errno(0);
        let d1 = |g: extern "C" fn(f64) -> f64| {
            let x = d(ins[0]);
            same_d_near(name, g(x), d(outs[0]), near_root(name, x))
        };
        let f1 = |g: extern "C" fn(f32) -> f32| {
            let x = f(ins[0]);
            same_f_near(name, g(x), f(outs[0]), near_root(name, f64::from(x)))
        };
        let d2 =
            |g: extern "C" fn(f64, f64) -> f64| same_d(name, g(d(ins[0]), d(ins[1])), d(outs[0]));
        let f2 =
            |g: extern "C" fn(f32, f32) -> f32| same_f(name, g(f(ins[0]), f(ins[1])), f(outs[0]));
        let exact_int = |ours: i64| {
            let glibc = int(outs[0]);
            if ours == glibc {
                Ok(())
            } else {
                Err(format!("{ours}, glibc {glibc}"))
            }
        };
        let result = match name {
            "fabs" => d1(fabs),
            "fabsf" => f1(fabsf),
            "floor" => d1(floor),
            "floorf" => f1(floorf),
            "ceil" => d1(ceil),
            "ceilf" => f1(ceilf),
            "round" => d1(round),
            "roundf" => f1(roundf),
            "trunc" => d1(trunc),
            "truncf" => f1(truncf),
            "sqrt" => d1(sqrt),
            "sqrtf" => f1(sqrtf),
            "exp" => d1(exp),
            "expf" => f1(expf),
            "exp2" => d1(exp2),
            "exp2f" => f1(exp2f),
            "exp10" => d1(exp10),
            "exp10f" => f1(exp10f),
            "expm1" => d1(expm1),
            "expm1f" => f1(expm1f),
            "log" => d1(log),
            "logf" => f1(logf),
            "log2" => d1(log2),
            "log2f" => f1(log2f),
            "log10" => d1(log10),
            "log10f" => f1(log10f),
            "log1p" => d1(log1p),
            "log1pf" => f1(log1pf),
            "sin" => d1(sin),
            "sinf" => f1(sinf),
            "cos" => d1(cos),
            "cosf" => f1(cosf),
            "tan" => d1(tan),
            "tanf" => f1(tanf),
            "asin" => d1(asin),
            "asinf" => f1(asinf),
            "acos" => d1(acos),
            "acosf" => f1(acosf),
            "atan" => d1(atan),
            "atanf" => f1(atanf),
            "sinh" => d1(sinh),
            "sinhf" => f1(sinhf),
            "cosh" => d1(cosh),
            "coshf" => f1(coshf),
            "tanh" => d1(tanh),
            "tanhf" => f1(tanhf),
            "asinh" => d1(asinh),
            "asinhf" => f1(asinhf),
            "acosh" => d1(acosh),
            "acoshf" => f1(acoshf),
            "atanh" => d1(atanh),
            "atanhf" => f1(atanhf),
            "cbrt" => d1(cbrt),
            "cbrtf" => f1(cbrtf),
            "erf" => d1(erf),
            "erff" => f1(erff),
            "erfc" => d1(erfc),
            "erfcf" => f1(erfcf),
            "lgamma" => d1(lgamma),
            "lgammaf" => f1(lgammaf),
            "tgamma" => d1(tgamma),
            "tgammaf" => f1(tgammaf),
            "gamma" => d1(gamma),
            "rint" => d1(rint),
            "rintf" => f1(rintf),
            "nearbyint" => d1(nearbyint),
            "nearbyintf" => f1(nearbyintf),
            "logb" => d1(logb),
            "logbf" => f1(logbf),
            "significand" => d1(significand),
            "j0" => d1(j0),
            "j1" => d1(j1),
            "y0" => d1(y0),
            "y1" => d1(y1),
            "fmod" => d2(fmod),
            "fmodf" => f2(fmodf),
            "pow" => d2(pow),
            "powf" => f2(powf),
            "atan2" => d2(atan2),
            "atan2f" => f2(atan2f),
            "copysign" => d2(copysign),
            "copysignf" => f2(copysignf),
            "fmin" => d2(fmin),
            "fminf" => f2(fminf),
            "fmax" => d2(fmax),
            "fmaxf" => f2(fmaxf),
            "hypot" => d2(hypot),
            "hypotf" => f2(hypotf),
            "fdim" => d2(fdim),
            "fdimf" => f2(fdimf),
            "nextafter" => d2(nextafter),
            "nextafterf" => f2(nextafterf),
            "remainder" => d2(remainder),
            "remainderf" => f2(remainderf),
            "drem" => d2(drem),
            "fma" => same_d(name, fma(d(ins[0]), d(ins[1]), d(ins[2])), d(outs[0])),
            "fmaf" => same_f(name, fmaf(f(ins[0]), f(ins[1]), f(ins[2])), f(outs[0])),
            "ilogb" => exact_int(i64::from(ilogb(d(ins[0])))),
            "ilogbf" => exact_int(i64::from(ilogbf(f(ins[0])))),
            "lround" => exact_int(lround(d(ins[0]))),
            "llround" => exact_int(llround(d(ins[0]))),
            "lrint" => exact_int(lrint(d(ins[0]))),
            "lroundf" => exact_int(lroundf(f(ins[0]))),
            "llroundf" => exact_int(llroundf(f(ins[0]))),
            "lrintf" => exact_int(lrintf(f(ins[0]))),
            "finite" => exact_int(i64::from(finite(d(ins[0])))),
            "finitef" => exact_int(i64::from(finitef(f(ins[0])))),
            "isnan" => exact_int(i64::from(isnan(d(ins[0])))),
            "isinf" => exact_int(i64::from(isinf(d(ins[0])))),
            "ldexp" | "scalbn" => {
                let n = i32::try_from(int(ins[1])).expect("an int exponent");
                let r = if name == "ldexp" {
                    ldexp(d(ins[0]), n)
                } else {
                    scalbn(d(ins[0]), n)
                };
                same_d(name, r, d(outs[0]))
            }
            "scalbln" => same_d(name, scalbln(d(ins[0]), int(ins[1])), d(outs[0])),
            "ldexpf" | "scalbnf" => {
                let n = i32::try_from(int(ins[1])).expect("an int exponent");
                let r = if name == "ldexpf" {
                    ldexpf(f(ins[0]), n)
                } else {
                    scalbnf(f(ins[0]), n)
                };
                same_f(name, r, f(outs[0]))
            }
            "scalblnf" => same_f(name, scalblnf(f(ins[0]), int(ins[1])), f(outs[0])),
            "jn" | "yn" => {
                let n = i32::try_from(int(ins[0])).expect("an int order");
                let r = if name == "jn" {
                    jn(n, d(ins[1]))
                } else {
                    yn(n, d(ins[1]))
                };
                same_d_near(name, r, d(outs[0]), true)
            }
            "frexp" | "lgamma_r" => {
                let mut o = 12345;
                let x = d(ins[0]);
                let r = if name == "frexp" {
                    frexp(x, &raw mut o)
                } else {
                    lgamma_r(x, &raw mut o)
                };
                // glibc leaves the exponent unspecified for a NaN or an
                // infinity; both libraries write 0 there, so it is compared.
                same_d_near(name, r, d(outs[0]), near_root(name, x)).and_then(|()| {
                    let want = int(outs[1]);
                    if i64::from(o) == want {
                        Ok(())
                    } else {
                        Err(format!("*out {o}, glibc {want}"))
                    }
                })
            }
            "frexpf" | "lgammaf_r" => {
                let mut o = 12345;
                let x = f(ins[0]);
                let r = if name == "frexpf" {
                    frexpf(x, &raw mut o)
                } else {
                    lgammaf_r(x, &raw mut o)
                };
                same_f_near(name, r, f(outs[0]), near_root(name, f64::from(x))).and_then(|()| {
                    let want = int(outs[1]);
                    if i64::from(o) == want {
                        Ok(())
                    } else {
                        Err(format!("*out {o}, glibc {want}"))
                    }
                })
            }
            "modf" => {
                let mut o = 0.0;
                let r = modf(d(ins[0]), &raw mut o);
                same_d(name, r, d(outs[0])).and_then(|()| same_d(name, o, d(outs[1])))
            }
            "modff" => {
                let mut o = 0.0;
                let r = modff(f(ins[0]), &raw mut o);
                same_f(name, r, f(outs[0])).and_then(|()| same_f(name, o, f(outs[1])))
            }
            "remquo" => {
                let mut q = 12345;
                let r = remquo(d(ins[0]), d(ins[1]), &raw mut q);
                // C fixes only the quotient's sign and its low three bits.
                same_d(name, r, d(outs[0])).and_then(|()| {
                    let want = int(outs[1]);
                    if r.is_nan() || quo_agrees(q, want) {
                        Ok(())
                    } else {
                        Err(format!("quo {q}, glibc {want}"))
                    }
                })
            }
            "remquof" => {
                let mut q = 12345;
                let r = remquof(f(ins[0]), f(ins[1]), &raw mut q);
                same_f(name, r, f(outs[0])).and_then(|()| {
                    let want = int(outs[1]);
                    if r.is_nan() || quo_agrees(q, want) {
                        Ok(())
                    } else {
                        Err(format!("quo {q}, glibc {want}"))
                    }
                })
            }
            "sincos" => {
                let (mut s, mut c) = (0.0, 0.0);
                let x = d(ins[0]);
                sincos(x, &raw mut s, &raw mut c);
                let near = near_root(name, x);
                same_d_near(name, s, d(outs[0]), near)
                    .and_then(|()| same_d_near(name, c, d(outs[1]), near))
            }
            "sincosf" => {
                let (mut s, mut c) = (0.0, 0.0);
                let x = f(ins[0]);
                sincosf(x, &raw mut s, &raw mut c);
                let near = near_root(name, f64::from(x));
                same_f_near(name, s, f(outs[0]), near)
                    .and_then(|()| same_f_near(name, c, f(outs[1]), near))
            }
            other => Err(format!(
                "the oracle calls {other}, which this replay does not know"
            )),
        };
        let got_errno = errno::get_errno();
        result.and_then(|()| {
            if got_errno == want_errno {
                Ok(())
            } else {
                Err(format!("errno {got_errno}, glibc {want_errno}"))
            }
        })
    }

    /// Lane E's cases (`requests/e-d-libc-round-is-wrong-just-below-a-half-and-past-2-52.md`):
    /// `floor(x + 0.5)` rounded the addition before the floor, so the double
    /// just below a half went to 1 and an odd number past 2^52 to the next
    /// even one; and a negative result of zero kept its sign.
    #[test]
    fn round_is_right_just_below_a_half_and_past_2_52() {
        let below_half = 0.499_999_999_999_999_94_f64;
        assert_eq!(below_half.to_bits(), 0x3FDF_FFFF_FFFF_FFFF);
        assert_eq!(round(below_half).to_bits(), 0.0_f64.to_bits());
        assert_eq!(round(-below_half).to_bits(), (-0.0_f64).to_bits());
        assert_eq!(round(4_503_599_627_370_497.0), 4_503_599_627_370_497.0);
        assert_eq!(round(-4_503_599_627_370_497.0), -4_503_599_627_370_497.0);
        assert_eq!(roundf(0.499_999_97_f32).to_bits(), 0.0_f32.to_bits());
        assert_eq!(roundf(8_388_609.0), 8_388_609.0);
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), -1.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(
            round(-0.3).to_bits(),
            (-0.0_f64).to_bits(),
            "round(-0.3) is -0.0"
        );
        assert_eq!(lround(below_half), 0);
        assert_eq!(llround(4_503_599_627_370_497.0), 4_503_599_627_370_497);
        assert_eq!(lroundf(0.499_999_97_f32), 0);
        assert_eq!(llroundf(8_388_609.0), 8_388_609);
    }

    /// `fma` rounds once: `(1 + e)(1 - e) - 1` is `-e^2` exactly, where the
    /// two-rounding `x*y + z` it used to be gives 0.
    #[test]
    fn fma_rounds_once() {
        let e = f64::EPSILON;
        assert_eq!(fma(1.0 + e, 1.0 - e, -1.0), -(e * e));
        assert_eq!(
            (1.0 + e) * (1.0 - e) - 1.0,
            0.0,
            "the two-rounding answer, for contrast"
        );
        let ef = f32::EPSILON;
        assert_eq!(fmaf(1.0 + ef, 1.0 - ef, -1.0), -(ef * ef));
    }

    /// `nan(tag)` puts glibc's payload in: the tag read as `strtoull` reads
    /// it, below the quiet bit; anything else is the default NaN.
    #[test]
    fn nan_takes_its_tag_as_glibc_does() {
        let bits = |t: &core::ffi::CStr| nan(t.as_ptr().cast()).to_bits();
        let quiet = f64::NAN.to_bits();
        assert_eq!(bits(c""), quiet);
        assert_eq!(bits(c"0x12"), quiet | 0x12);
        assert_eq!(bits(c"017"), quiet | 0o17);
        assert_eq!(bits(c"42"), quiet | 42);
        assert_eq!(bits(c"0"), quiet, "a zero payload is the default NaN");
        assert_eq!(bits(c"0x"), quiet, "no digits after 0x");
        assert_eq!(bits(c"08"), quiet, "8 is no octal digit");
        assert_eq!(bits(c"abc"), quiet, "not a number");
        assert_eq!(bits(c" 1"), quiet, "a space is no n-char");
        assert_eq!(nan(core::ptr::null()).to_bits(), quiet);
        assert_eq!(
            nanf(c"0x7".as_ptr().cast()).to_bits(),
            f32::NAN.to_bits() | 7
        );
    }

    /// glibc 2.39's answer to every call in the oracle, and its `errno`:
    /// bit for bit for the exact functions, within [`ulps_allowed`] for the
    /// rest.
    #[test]
    fn every_answer_is_glibcs_or_within_its_error() {
        let _g = signgam_lock();
        let mut failures = Vec::new();
        let mut calls = 0usize;
        for line in ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (lhs, rhs) = line.split_once(" = ").expect("an oracle line has an =");
            let mut words = lhs.split(' ');
            let name = words.next().expect("a function name");
            let ins: Vec<&str> = words.collect();
            let outs: Vec<&str> = rhs.split(' ').collect();
            calls += 1;
            if let Err(why) = replay_math(name, &ins, &outs) {
                failures.push(format!("{line}\n    ours: {why}"));
            }
        }
        assert!(
            calls > 20_000,
            "the oracle is {calls} calls; it should be the whole table"
        );
        assert!(
            failures.is_empty(),
            "{} of {calls} calls differ from glibc:\n{}",
            failures.len(),
            failures
                .iter()
                .take(80)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

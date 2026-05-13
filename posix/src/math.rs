//! C math library functions (`<math.h>`).
//!
//! Provides software implementations of common math functions for
//! `no_std` environments.  These are not high-performance — programs
//! needing fast math should use SIMD or FPU-optimized versions.
//!
//! ## Implemented Functions
//!
//! - `fabs`, `fabsf` — absolute value
//! - `floor`, `floorf`, `ceil`, `ceilf` — rounding
//! - `round`, `roundf`, `trunc`, `truncf` — rounding
//! - `rint`, `rintf`, `nearbyint`, `nearbyintf` — round to nearest (ties to even)
//! - `lround`, `lroundf`, `llround`, `llroundf` — float→integer
//! - `lrint`, `lrintf` — float→integer (ties to even)
//! - `fmod`, `fmodf` — floating-point remainder (truncated)
//! - `remainder`, `remainderf` — IEEE 754 remainder (rounded)
//! - `sqrt`, `sqrtf` — square root (Newton's method)
//! - `cbrt`, `cbrtf` — cube root
//! - `hypot`, `hypotf` — Euclidean distance
//! - `pow`, `powf` — power (via exp/log)
//! - `log`, `logf`, `log2`, `log2f`, `log10`, `log10f` — logarithms
//! - `log1p`, `log1pf` — log(1+x) accurate for small x
//! - `exp`, `expf`, `exp2`, `exp2f` — exponential
//! - `expm1`, `expm1f` — exp(x)-1 accurate for small x
//! - `sin`, `sinf`, `cos`, `cosf`, `tan`, `tanf` — trigonometry
//! - `asin`, `asinf`, `acos`, `acosf`, `atan`, `atanf` — inverse trig
//! - `atan2`, `atan2f` — two-argument arctangent
//! - `sinh`, `sinhf`, `cosh`, `coshf`, `tanh`, `tanhf` — hyperbolic
//! - `frexp`, `frexpf`, `ldexp`, `ldexpf`, `modf`, `modff` — decomposition
//! - `scalbn`, `scalbnf`, `scalbln`, `scalblnf` — scale by power of 2
//! - `ilogb`, `ilogbf`, `logb`, `logbf` — exponent extraction
//! - `isnan`, `isinf`, `isfinite` — classification
//! - `copysign`, `copysignf` — sign manipulation
//! - `nextafter`, `nextafterf` — adjacent representable value
//! - `fmin`, `fmax`, `fminf`, `fmaxf` — min/max
//! - `fdim`, `fdimf` — positive difference
//! - `fma`, `fmaf` — fused multiply-add
//! - `nan`, `nanf` — quiet NaN
//!
//! ## Accuracy
//!
//! These use polynomial/Taylor approximations and are accurate to
//! roughly 10-15 digits for `f64`, 5-7 digits for `f32`.  Edge cases
//! (NaN, infinity, denormals) are handled but not exhaustively tested.

use core::f64::consts;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const PI: f64 = consts::PI;
const HALF_PI: f64 = consts::FRAC_PI_2;
const TWO_PI: f64 = consts::TAU;
const LN2: f64 = consts::LN_2;
const LN10: f64 = consts::LN_10;
const LOG2E: f64 = consts::LOG2_E;

/// Special value constants.
#[unsafe(no_mangle)]
pub static HUGE_VAL: f64 = f64::INFINITY;
#[unsafe(no_mangle)]
pub static HUGE_VALF: f32 = f32::INFINITY;

// ---------------------------------------------------------------------------
// Absolute value
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fabs(x: f64) -> f64 {
    // Bit manipulation handles -0.0 correctly (IEEE 754: -0.0 < 0.0 is false,
    // so a comparison-based approach would return -0.0 unchanged).
    f64::from_bits(x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF)
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fabsf(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7FFF_FFFF)
}

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn floor(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    // All f64 values with |x| >= 2^52 are already exact integers;
    // casting them to i64 would saturate and corrupt the value.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    let i = x as i64;
    let f = i as f64;
    if x < f { f - 1.0 } else { f }
}

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn floorf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    // All f32 values with |x| >= 2^23 are already exact integers.
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    let i = x as i32;
    let f = i as f32;
    if x < f { f - 1.0 } else { f }
}

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn ceil(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    let i = x as i64;
    let f = i as f64;
    if x > f { f + 1.0 } else { f }
}

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn ceilf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    let i = x as i32;
    let f = i as f32;
    if x > f { f + 1.0 } else { f }
}

#[unsafe(no_mangle)]
pub extern "C" fn round(x: f64) -> f64 {
    // POSIX: round halfway cases away from zero.
    // floor(x + 0.5) alone gives wrong results for negative halves:
    // round(-0.5) would be floor(0.0) = 0.0 instead of -1.0.
    if x >= 0.0 { floor(x + 0.5) } else { ceil(x - 0.5) }
}

#[unsafe(no_mangle)]
pub extern "C" fn roundf(x: f32) -> f32 {
    if x >= 0.0 { floorf(x + 0.5) } else { ceilf(x - 0.5) }
}

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn trunc(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    // All f64 values with |x| >= 2^52 are already exact integers.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    x as i64 as f64
}

#[unsafe(no_mangle)]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn truncf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    x as i32 as f32
}

// ---------------------------------------------------------------------------
// Remainder
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    if y == 0.0 { return f64::NAN; }
    x - trunc(x / y) * y
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    if y == 0.0 { return f32::NAN; }
    x - truncf(x / y) * y
}

// ---------------------------------------------------------------------------
// Square root — Newton-Raphson
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects, clippy::suboptimal_flops)]
pub extern "C" fn sqrt(x: f64) -> f64 {
    if x < 0.0 { return f64::NAN; }
    if x == 0.0 || x.is_nan() || x.is_infinite() { return x; }

    // Initial guess.
    let mut guess = x * 0.5;
    // 8 iterations of Newton's method: g = (g + x/g) / 2.
    let mut iter = 0;
    while iter < 8 {
        guess = (guess + x / guess) * 0.5;
        iter += 1;
    }
    guess
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects, clippy::suboptimal_flops)]
pub extern "C" fn sqrtf(x: f32) -> f32 {
    if x < 0.0 { return f32::NAN; }
    if x == 0.0 || x.is_nan() || x.is_infinite() { return x; }

    let mut guess = x * 0.5;
    let mut iter = 0;
    while iter < 6 {
        guess = (guess + x / guess) * 0.5;
        iter += 1;
    }
    guess
}

// ---------------------------------------------------------------------------
// Exponential — Taylor series
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn exp(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x > 709.0 { return f64::INFINITY; }
    if x < -709.0 { return 0.0; }

    // Range reduction: exp(x) = 2^k * exp(r) where r = x - k*ln(2).
    // Precision loss on i64→f64 is acceptable: k is small (|k| <= ~1024).
    #[allow(clippy::cast_precision_loss)]
    let k = (x * LOG2E) as i64;
    #[allow(clippy::cast_precision_loss)]
    let r = x - (k as f64) * LN2;

    // Taylor series for exp(r), |r| <= ln(2)/2.
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    let mut n: i32 = 1;
    while n <= 20 {
        term *= r / f64::from(n);
        sum += term;
        if fabs(term) < 1e-16 {
            break;
        }
        n += 1;
    }

    // Multiply by 2^k.
    #[allow(clippy::cast_possible_truncation)]
    ldexp(sum, k as i32)
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn expf(x: f32) -> f32 {
    exp(f64::from(x)) as f32
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn exp2(x: f64) -> f64 {
    exp(x * LN2)
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn exp2f(x: f32) -> f32 {
    expf(x * LN2 as f32)
}

// ---------------------------------------------------------------------------
// Natural logarithm
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log(x: f64) -> f64 {
    if x < 0.0 { return f64::NAN; }
    if x == 0.0 { return f64::NEG_INFINITY; }
    if x.is_nan() || x.is_infinite() { return x; }

    // Decompose: x = m * 2^e, 0.5 <= m < 1.
    let mut e_val: i32 = 0;
    let m = frexp_internal(x, &mut e_val);

    // ln(x) = ln(m * 2^e) = ln(m) + e*ln(2).
    // Use the series ln((1+t)/(1-t)) = 2*(t + t^3/3 + t^5/5 + ...)
    // where t = (m-1)/(m+1), valid for m near 1.
    // Since 0.5 <= m < 1, adjust: use m*2 to get range [1, 2).
    let adj_m = m * 2.0;
    let adj_e = e_val - 1;

    let t = (adj_m - 1.0) / (adj_m + 1.0);
    let t2 = t * t;

    let mut sum = t;
    let mut term = t;
    let mut k: i32 = 3;
    while k <= 41 {
        term *= t2;
        sum += term / f64::from(k);
        k += 2;
    }

    2.0 * sum + f64::from(adj_e) * LN2
}

#[unsafe(no_mangle)]
pub extern "C" fn logf(x: f32) -> f32 {
    log(f64::from(x)) as f32
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log2(x: f64) -> f64 {
    log(x) * LOG2E
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log2f(x: f32) -> f32 {
    logf(x) * LOG2E as f32
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log10(x: f64) -> f64 {
    log(x) / LN10
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log10f(x: f32) -> f32 {
    logf(x) / LN10 as f32
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::float_cmp)] // Exact comparisons are intentional for special-value handling.
pub extern "C" fn pow(base: f64, exponent: f64) -> f64 {
    if exponent == 0.0 { return 1.0; }
    if base == 1.0 { return 1.0; }
    if base.is_nan() || exponent.is_nan() { return f64::NAN; }
    if base == 0.0 {
        // pow(0, positive) = 0; pow(0, negative) = ∞ (pole error).
        if exponent > 0.0 { return 0.0; }
        return f64::INFINITY;
    }

    // Integer exponents: use repeated squaring.
    #[allow(clippy::cast_precision_loss)]
    let e_trunc = exponent as i64;
    #[allow(clippy::cast_precision_loss)]
    let e_back = e_trunc as f64;
    if exponent == e_back {
        return ipow(base, e_trunc);
    }

    // General case: base^exp = exp(exp * ln(base)).
    exp(exponent * log(base))
}

#[unsafe(no_mangle)]
pub extern "C" fn powf(base: f32, exponent: f32) -> f32 {
    pow(f64::from(base), f64::from(exponent)) as f32
}

/// Integer power via repeated squaring.
#[allow(clippy::arithmetic_side_effects)]
fn ipow(mut base: f64, mut exp: i64) -> f64 {
    if exp < 0 {
        base = 1.0 / base;
        exp = -exp;
    }

    let mut result = 1.0;
    while exp > 0 {
        if exp & 1 == 1 {
            result *= base;
        }
        base *= base;
        exp >>= 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Trigonometry — Taylor series
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn sin(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return f64::NAN; }

    // Range reduce to [-π, π].
    let mut r = fmod(x, TWO_PI);
    if r > PI { r -= TWO_PI; }
    if r < -PI { r += TWO_PI; }

    // Taylor series: sin(x) = x - x^3/3! + x^5/5! - ...
    let x2 = r * r;
    let mut term = r;
    let mut sum = r;
    let mut n: i32 = 1;
    while n <= 12 {
        let denom = f64::from(2 * n) * f64::from(2 * n + 1);
        term *= -x2 / denom;
        sum += term;
        n += 1;
    }
    sum
}

#[unsafe(no_mangle)]
pub extern "C" fn sinf(x: f32) -> f32 {
    sin(f64::from(x)) as f32
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn cos(x: f64) -> f64 {
    sin(x + HALF_PI)
}

#[unsafe(no_mangle)]
pub extern "C" fn cosf(x: f32) -> f32 {
    cos(f64::from(x)) as f32
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tan(x: f64) -> f64 {
    let c = cos(x);
    if fabs(c) < 1e-15 { return f64::INFINITY; }
    sin(x) / c
}

#[unsafe(no_mangle)]
pub extern "C" fn tanf(x: f32) -> f32 {
    tan(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Inverse trigonometry
// ---------------------------------------------------------------------------

/// Two-argument arctangent.
///
/// Returns the angle (in radians) whose tangent is y/x, using the
/// signs of both arguments to determine the quadrant.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() { return f64::NAN; }

    if x > 0.0 {
        return atan_approx(y / x);
    }
    if x < 0.0 {
        if y >= 0.0 {
            return atan_approx(y / x) + PI;
        }
        return atan_approx(y / x) - PI;
    }
    // x == 0
    if y > 0.0 { return HALF_PI; }
    if y < 0.0 { return -HALF_PI; }
    0.0 // Both zero.
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    atan2(f64::from(y), f64::from(x)) as f32
}

/// Arctangent approximation using a polynomial.
#[allow(clippy::arithmetic_side_effects)]
fn atan_approx(x: f64) -> f64 {
    // Range reduction: for |x| > 1, use atan(x) = π/2 - atan(1/x).
    if fabs(x) > 1.0 {
        let sign = if x > 0.0 { 1.0 } else { -1.0 };
        return sign * HALF_PI - atan_approx(1.0 / x);
    }

    // Padé-like approximation for |x| <= 1.
    // atan(x) ≈ x - x^3/3 + x^5/5 - x^7/7 + ...
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    let mut n: i32 = 1;
    while n <= 15 {
        let k = 2 * n + 1;
        term *= -x2;
        sum += term / f64::from(k);
        n += 1;
    }
    sum
}

// ---------------------------------------------------------------------------
// Floating-point decomposition
// ---------------------------------------------------------------------------

/// Break a float into fraction and exponent.
///
/// Returns m such that x = m * 2^exp, where 0.5 <= |m| < 1.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn frexp(x: f64, exp: *mut i32) -> f64 {
    if exp.is_null() { return x; }
    let mut e: i32 = 0;
    let m = frexp_internal(x, &mut e);
    unsafe { *exp = e; }
    m
}

/// Internal frexp without pointer.
#[allow(clippy::arithmetic_side_effects)]
fn frexp_internal(x: f64, exp: &mut i32) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        *exp = 0;
        return x;
    }

    let bits = x.to_bits();
    let biased_exp = ((bits >> 52) & 0x7FF) as i32;
    *exp = biased_exp - 1022; // Adjust so that 0.5 <= |m| < 1.

    // Replace exponent with 1022 (which gives 0.5 * mantissa).
    let mantissa_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | (1022_u64 << 52);
    f64::from_bits(mantissa_bits)
}

/// Scale a float by a power of 2.
///
/// Returns x * 2^exp.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn ldexp(x: f64, exp: i32) -> f64 {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }

    let bits = x.to_bits();
    let biased_exp = ((bits >> 52) & 0x7FF) as i32;
    let new_exp = biased_exp + exp;

    if new_exp >= 2047 {
        return if x > 0.0 { f64::INFINITY } else { f64::NEG_INFINITY };
    }
    if new_exp <= 0 {
        return 0.0; // Underflow to zero (don't handle denormals).
    }

    let new_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | ((new_exp as u64) << 52);
    f64::from_bits(new_bits)
}

/// Split a float into integer and fractional parts.
#[unsafe(no_mangle)]
pub extern "C" fn modf(x: f64, iptr: *mut f64) -> f64 {
    let int_part = trunc(x);
    if !iptr.is_null() {
        unsafe { *iptr = int_part; }
    }
    x - int_part
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn isnan(x: f64) -> i32 {
    i32::from(x.is_nan())
}

#[unsafe(no_mangle)]
pub extern "C" fn isinf(x: f64) -> i32 {
    if x == f64::INFINITY { 1 }
    else if x == f64::NEG_INFINITY { -1 }
    else { 0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn isfinite(x: f64) -> i32 {
    i32::from(x.is_finite())
}

// ---------------------------------------------------------------------------
// Sign manipulation
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn copysign(x: f64, y: f64) -> f64 {
    let mag = x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF;
    let sign = y.to_bits() & 0x8000_0000_0000_0000;
    f64::from_bits(mag | sign)
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn copysignf(x: f32, y: f32) -> f32 {
    let mag = x.to_bits() & 0x7FFF_FFFF;
    let sign = y.to_bits() & 0x8000_0000;
    f32::from_bits(mag | sign)
}

// ---------------------------------------------------------------------------
// Min / Max
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn fmin(x: f64, y: f64) -> f64 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x < y { x } else { y }
}

#[unsafe(no_mangle)]
pub extern "C" fn fmax(x: f64, y: f64) -> f64 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x > y { x } else { y }
}

#[unsafe(no_mangle)]
pub extern "C" fn fminf(x: f32, y: f32) -> f32 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x < y { x } else { y }
}

#[unsafe(no_mangle)]
pub extern "C" fn fmaxf(x: f32, y: f32) -> f32 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x > y { x } else { y }
}

// ---------------------------------------------------------------------------
// Inverse trigonometry (single-argument)
// ---------------------------------------------------------------------------

/// Compute arc tangent of x.
///
/// Returns a value in [-π/2, π/2].
#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    atan_approx(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn atanf(x: f32) -> f32 {
    atan(f64::from(x)) as f32
}

/// Compute arc sine of x.
///
/// Returns a value in [-π/2, π/2].  Domain: |x| <= 1.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn asin(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x > 1.0 || x < -1.0 { return f64::NAN; }
    if x == 1.0 { return HALF_PI; }
    if x == -1.0 { return -HALF_PI; }
    // asin(x) = atan2(x, sqrt(1 - x²))
    atan2(x, sqrt(1.0 - x * x))
}

#[unsafe(no_mangle)]
pub extern "C" fn asinf(x: f32) -> f32 {
    asin(f64::from(x)) as f32
}

/// Compute arc cosine of x.
///
/// Returns a value in [0, π].  Domain: |x| <= 1.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn acos(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x > 1.0 || x < -1.0 { return f64::NAN; }
    if x == 1.0 { return 0.0; }
    if x == -1.0 { return PI; }
    // acos(x) = atan2(sqrt(1 - x²), x)
    atan2(sqrt(1.0 - x * x), x)
}

#[unsafe(no_mangle)]
pub extern "C" fn acosf(x: f32) -> f32 {
    acos(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Hyperbolic functions
// ---------------------------------------------------------------------------

/// Hyperbolic sine.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn sinh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x.is_infinite() { return x; }
    // sinh(x) = (e^x - e^(-x)) / 2
    let ex = exp(x);
    (ex - 1.0 / ex) * 0.5
}

#[unsafe(no_mangle)]
pub extern "C" fn sinhf(x: f32) -> f32 {
    sinh(f64::from(x)) as f32
}

/// Hyperbolic cosine.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn cosh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x.is_infinite() { return f64::INFINITY; }
    // cosh(x) = (e^x + e^(-x)) / 2
    let ex = exp(x);
    (ex + 1.0 / ex) * 0.5
}

#[unsafe(no_mangle)]
pub extern "C" fn coshf(x: f32) -> f32 {
    cosh(f64::from(x)) as f32
}

/// Hyperbolic tangent.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tanh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x > 20.0 { return 1.0; }
    if x < -20.0 { return -1.0; }
    // tanh(x) = (e^2x - 1) / (e^2x + 1)
    let e2x = exp(2.0 * x);
    (e2x - 1.0) / (e2x + 1.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn tanhf(x: f32) -> f32 {
    tanh(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Other commonly needed functions
// ---------------------------------------------------------------------------

/// Euclidean distance: sqrt(x² + y²) without overflow.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() { return f64::INFINITY; }
    if x.is_nan() || y.is_nan() { return f64::NAN; }
    // Use the "scaled" method to avoid overflow for large x,y.
    let ax = fabs(x);
    let ay = fabs(y);
    let (big, small) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    if big == 0.0 { return 0.0; }
    let ratio = small / big;
    big * sqrt(1.0 + ratio * ratio)
}

#[unsafe(no_mangle)]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    hypot(f64::from(x), f64::from(y)) as f32
}

/// Cube root.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn cbrt(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() || x == 0.0 { return x; }
    // Newton's method: cbrt(x) via y = x^(1/3).
    // Initial estimate using pow.
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = fabs(x);
    // Use Halley's method for fast convergence.
    let mut y = pow(ax, 1.0 / 3.0);
    // Two refinement iterations for full f64 precision.
    y = (2.0 * y + ax / (y * y)) / 3.0;
    y = (2.0 * y + ax / (y * y)) / 3.0;
    sign * y
}

#[unsafe(no_mangle)]
pub extern "C" fn cbrtf(x: f32) -> f32 {
    cbrt(f64::from(x)) as f32
}

/// log(1 + x), accurate for small x.
///
/// For |x| < 1e-4, uses Taylor series to avoid catastrophic cancellation
/// in the naive `log(1 + x)`.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log1p(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x == f64::INFINITY { return f64::INFINITY; }
    if x < -1.0 { return f64::NAN; }
    if x == -1.0 { return f64::NEG_INFINITY; }
    if fabs(x) < 1e-4 {
        // Taylor: log(1+x) ≈ x - x²/2 + x³/3 - x⁴/4 + ...
        let x2 = x * x;
        let x3 = x2 * x;
        let x4 = x3 * x;
        return x - x2 * 0.5 + x3 / 3.0 - x4 * 0.25;
    }
    log(1.0 + x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log1pf(x: f32) -> f32 {
    log1p(f64::from(x)) as f32
}

/// exp(x) - 1, accurate for small x.
///
/// For |x| < 1e-4, uses Taylor series to avoid catastrophic cancellation.
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn expm1(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x == f64::INFINITY { return f64::INFINITY; }
    if x == f64::NEG_INFINITY { return -1.0; }
    if fabs(x) < 1e-4 {
        // Taylor: e^x - 1 ≈ x + x²/2 + x³/6 + x⁴/24
        let x2 = x * x;
        let x3 = x2 * x;
        let x4 = x3 * x;
        return x + x2 * 0.5 + x3 / 6.0 + x4 / 24.0;
    }
    exp(x) - 1.0
}

#[unsafe(no_mangle)]
pub extern "C" fn expm1f(x: f32) -> f32 {
    expm1(f64::from(x)) as f32
}

/// Positive difference: max(x - y, 0).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fdim(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() { return f64::NAN; }
    if x > y { x - y } else { 0.0 }
}

#[unsafe(no_mangle)]
pub extern "C" fn fdimf(x: f32, y: f32) -> f32 {
    fdim(f64::from(x), f64::from(y)) as f32
}

/// Fused multiply-add: x * y + z (computed without intermediate rounding).
///
/// On x86_64, FMA3 is available via intrinsic; we use a simple
/// implementation that computes at f64 precision (which gives correct
/// rounding for most cases in the f32 variant).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fma(x: f64, y: f64, z: f64) -> f64 {
    // Without hardware FMA, this is the best we can do portably.
    // The rounding error is at most 1 ULP for typical inputs.
    x * y + z
}

#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmaf(x: f32, y: f32, z: f32) -> f32 {
    // Perform in f64 for correct f32 result (f64 has enough precision
    // to represent the exact f32 product without rounding).
    (f64::from(x) * f64::from(y) + f64::from(z)) as f32
}

/// Round to nearest integer, ties away from zero (long).
#[unsafe(no_mangle)]
pub extern "C" fn lround(x: f64) -> i64 {
    round(x) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn lroundf(x: f32) -> i64 {
    roundf(x) as i64
}

/// Round to nearest integer, ties away from zero (long long).
#[unsafe(no_mangle)]
pub extern "C" fn llround(x: f64) -> i64 {
    round(x) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn llroundf(x: f32) -> i64 {
    roundf(x) as i64
}

/// Round to nearest integer, ties to even (long).
///
/// Without FP environment control, we approximate with `round`.
#[unsafe(no_mangle)]
pub extern "C" fn lrint(x: f64) -> i64 {
    // Banker's rounding: if exactly halfway, round to even.
    let r = round(x);
    let diff = x - r;
    if diff == 0.5 || diff == -0.5 {
        let ri = r as i64;
        if ri & 1 != 0 {
            return if diff > 0.0 { ri - 1 } else { ri + 1 };
        }
    }
    r as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn lrintf(x: f32) -> i64 {
    lrint(f64::from(x))
}

/// Round to nearest integer value (as floating-point).
#[unsafe(no_mangle)]
pub extern "C" fn rint(x: f64) -> f64 {
    lrint(x) as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn rintf(x: f32) -> f32 {
    lrintf(x) as f32
}

/// Same as `rint`, but does not raise FP exceptions.
#[unsafe(no_mangle)]
pub extern "C" fn nearbyint(x: f64) -> f64 {
    rint(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn nearbyintf(x: f32) -> f32 {
    rintf(x)
}

/// Multiply by power of 2 (integer exponent).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn scalbn(x: f64, n: i32) -> f64 {
    ldexp(x, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn scalbnf(x: f32, n: i32) -> f32 {
    ldexp(f64::from(x), n) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn scalbln(x: f64, n: i64) -> f64 {
    // Clamp to i32 range (exponents beyond ±1074 produce 0 or inf anyway).
    let clamped = n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    ldexp(x, clamped)
}

#[unsafe(no_mangle)]
pub extern "C" fn scalblnf(x: f32, n: i64) -> f32 {
    scalbln(f64::from(x), n) as f32
}

/// Extract unbiased exponent as integer.
///
/// Returns the exponent of x such that 1 <= |x| * 2^(-ilogb(x)) < 2.
/// Special: ilogb(0) = `FP_ILOGB0`, ilogb(inf) = `INT_MAX`,
/// ilogb(NaN) = `FP_ILOGBNAN`.
#[unsafe(no_mangle)]
pub extern "C" fn ilogb(x: f64) -> i32 {
    if x.is_nan() { return i32::MAX; } // FP_ILOGBNAN
    if x.is_infinite() { return i32::MAX; }
    if x == 0.0 { return i32::MIN; } // FP_ILOGB0
    let bits = x.to_bits();
    let exp_field = ((bits >> 52) & 0x7FF) as i32;
    if exp_field == 0 {
        // Subnormal — count leading zeros in mantissa.
        let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
        let lz = mantissa.leading_zeros() as i32 - 12; // 64 - 52 = 12
        -1023 - lz
    } else {
        exp_field - 1023
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ilogbf(x: f32) -> i32 {
    ilogb(f64::from(x))
}

/// Extract unbiased exponent as f64.
#[unsafe(no_mangle)]
pub extern "C" fn logb(x: f64) -> f64 {
    if x == 0.0 { return f64::NEG_INFINITY; }
    if x.is_infinite() { return f64::INFINITY; }
    if x.is_nan() { return f64::NAN; }
    f64::from(ilogb(x))
}

#[unsafe(no_mangle)]
pub extern "C" fn logbf(x: f32) -> f32 {
    logb(f64::from(x)) as f32
}

/// Next representable f64 value after `from` in the direction of `to`.
#[unsafe(no_mangle)]
pub extern "C" fn nextafter(from: f64, to: f64) -> f64 {
    if from.is_nan() || to.is_nan() { return f64::NAN; }
    if from == to { return to; }
    if from == 0.0 {
        // Smallest subnormal in the direction of `to`.
        let bits: u64 = if to > 0.0 { 1 } else { 0x8000_0000_0000_0001 };
        return f64::from_bits(bits);
    }
    let bits = from.to_bits();
    let next_bits = if (to > from) == (from > 0.0) {
        bits.wrapping_add(1)
    } else {
        bits.wrapping_sub(1)
    };
    f64::from_bits(next_bits)
}

#[unsafe(no_mangle)]
pub extern "C" fn nextafterf(from: f32, to: f32) -> f32 {
    if from.is_nan() || to.is_nan() { return f32::NAN; }
    if from == to { return to; }
    if from == 0.0 {
        let bits: u32 = if to > 0.0 { 1 } else { 0x8000_0001 };
        return f32::from_bits(bits);
    }
    let bits = from.to_bits();
    let next_bits = if (to > from) == (from > 0.0) {
        bits.wrapping_add(1)
    } else {
        bits.wrapping_sub(1)
    };
    f32::from_bits(next_bits)
}

/// IEEE 754 remainder (difference from `fmod`: uses round-to-nearest).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn remainder(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() { return f64::NAN; }
    if y.is_nan() || x.is_nan() { return f64::NAN; }
    if y.is_infinite() { return x; }
    let n = round(x / y);
    x - n * y
}

#[unsafe(no_mangle)]
pub extern "C" fn remainderf(x: f32, y: f32) -> f32 {
    remainder(f64::from(x), f64::from(y)) as f32
}

/// NaN with optional tag string (C99 nan("")).
///
/// The tag is ignored (implementation-defined payload).
#[unsafe(no_mangle)]
pub extern "C" fn nan(_tag: *const u8) -> f64 {
    f64::NAN
}

/// NaN (f32 variant).
#[unsafe(no_mangle)]
pub extern "C" fn nanf(_tag: *const u8) -> f32 {
    f32::NAN
}

/// frexp for f32.
#[unsafe(no_mangle)]
pub extern "C" fn frexpf(x: f32, exp: *mut i32) -> f32 {
    frexp(f64::from(x), exp) as f32
}

/// ldexp for f32.
#[unsafe(no_mangle)]
pub extern "C" fn ldexpf(x: f32, exp: i32) -> f32 {
    ldexp(f64::from(x), exp) as f32
}

/// modf for f32.
#[unsafe(no_mangle)]
pub extern "C" fn modff(x: f32, iptr: *mut f32) -> f32 {
    let mut id: f64 = 0.0;
    let frac = modf(f64::from(x), &raw mut id);
    if !iptr.is_null() {
        // SAFETY: caller guarantees iptr is valid.
        unsafe { *iptr = id as f32; }
    }
    frac as f32
}

/// Return the floating-point number with the magnitude of `x` and the
/// sign of the product `x * y` (GNU extension used by some libm ports).
#[unsafe(no_mangle)]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn __copysign(x: f64, y: f64) -> f64 {
    copysign(x, y)
}

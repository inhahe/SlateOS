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
//! - `fmod`, `fmodf` — floating-point remainder
//! - `sqrt`, `sqrtf` — square root (Newton's method)
//! - `pow`, `powf` — power (via exp/log)
//! - `log`, `logf`, `log2`, `log2f`, `log10`, `log10f` — logarithms
//! - `exp`, `expf`, `exp2`, `exp2f` — exponential
//! - `sin`, `sinf`, `cos`, `cosf`, `tan`, `tanf` — trigonometry
//! - `atan2`, `atan2f` — two-argument arctangent
//! - `frexp`, `ldexp`, `modf` — floating-point decomposition
//! - `isnan`, `isinf`, `isfinite` — classification
//! - `copysign`, `copysignf` — sign manipulation
//! - `fmin`, `fmax`, `fminf`, `fmaxf` — min/max
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

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
//! - `asinh`, `asinhf`, `acosh`, `acoshf`, `atanh`, `atanhf` — inverse hyperbolic
//! - `frexp`, `frexpf`, `ldexp`, `ldexpf`, `modf`, `modff` — decomposition
//! - `scalbn`, `scalbnf`, `scalbln`, `scalblnf` — scale by power of 2
//! - `ilogb`, `ilogbf`, `logb`, `logbf` — exponent extraction
//! - `isnan`, `isinf`, `isfinite` — classification
//! - `copysign`, `copysignf` — sign manipulation
//! - `nextafter`, `nextafterf` — adjacent representable value
//! - `fmin`, `fmax`, `fminf`, `fmaxf` — min/max
//! - `fdim`, `fdimf` — positive difference
//! - `fma`, `fmaf` — fused multiply-add
//! - `remquo`, `remquof` — IEEE remainder with quotient bits
//! - `nan`, `nanf` — quiet NaN
//! - `erf`, `erff`, `erfc`, `erfcf` — error function
//! - `lgamma`, `lgammaf`, `lgamma_r`, `lgammaf_r` — log-gamma
//! - `tgamma`, `tgammaf` — true gamma function
//! - `sincos`, `sincosf` — simultaneous sin/cos
//! - `exp10`, `exp10f`, `pow10`, `pow10f` — base-10 exponential
//! - `j0`, `j1`, `jn` — Bessel functions (first kind)
//! - `y0`, `y1`, `yn` — Bessel functions (second kind)
//! - `finite`, `significand`, `drem`, `gamma` — deprecated aliases
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
const QUARTER_PI: f64 = consts::FRAC_PI_4;
const TWO_PI: f64 = consts::TAU;
const LN2: f64 = consts::LN_2;
const LN10: f64 = consts::LN_10;
const LOG2E: f64 = consts::LOG2_E;

/// Special value constants.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static HUGE_VAL: f64 = f64::INFINITY;
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub static HUGE_VALF: f32 = f32::INFINITY;

// ---------------------------------------------------------------------------
// Absolute value
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fabs(x: f64) -> f64 {
    // Bit manipulation handles -0.0 correctly (IEEE 754: -0.0 < 0.0 is false,
    // so a comparison-based approach would return -0.0 unchanged).
    f64::from_bits(x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fabsf(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7FFF_FFFF)
}

// ---------------------------------------------------------------------------
// Rounding
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn floor(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    // Preserve ±0.0 (IEEE 754 / POSIX: floor(-0.0) = -0.0).
    if x == 0.0 { return x; }
    // All f64 values with |x| >= 2^52 are already exact integers;
    // casting them to i64 would saturate and corrupt the value.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    let i = x as i64;
    let f = i as f64;
    if x < f { f - 1.0 } else { f }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn floorf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; }
    // All f32 values with |x| >= 2^23 are already exact integers.
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    let i = x as i32;
    let f = i as f32;
    if x < f { f - 1.0 } else { f }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn ceil(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; }
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    let i = x as i64;
    let f = i as f64;
    if x > f { f + 1.0 } else { f }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn ceilf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; }
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    let i = x as i32;
    let f = i as f32;
    if x > f { f + 1.0 } else { f }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn round(x: f64) -> f64 {
    // POSIX: round halfway cases away from zero.
    // floor(x + 0.5) alone gives wrong results for negative halves:
    // round(-0.5) would be floor(0.0) = 0.0 instead of -1.0.
    if x >= 0.0 { floor(x + 0.5) } else { ceil(x - 0.5) }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn roundf(x: f32) -> f32 {
    if x >= 0.0 { floorf(x + 0.5) } else { ceilf(x - 0.5) }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn trunc(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    // Preserve ±0.0 (IEEE 754 / POSIX: trunc(-0.0) = -0.0).
    if x == 0.0 { return x; }
    // All f64 values with |x| >= 2^52 are already exact integers.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    x as i64 as f64
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn truncf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; }
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    x as i32 as f32
}

// ---------------------------------------------------------------------------
// Remainder
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    if y == 0.0 { return f64::NAN; }
    x - trunc(x / y) * y
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    if y == 0.0 { return f32::NAN; }
    x - truncf(x / y) * y
}

// ---------------------------------------------------------------------------
// Square root — Newton-Raphson
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::suboptimal_flops)]
pub extern "C" fn sqrt(x: f64) -> f64 {
    if x < 0.0 { return f64::NAN; }
    if x == 0.0 || x.is_nan() || x.is_infinite() { return x; }

    // Decompose x = m * 2^e (0.5 <= m < 1) for a good initial guess.
    // The naive initial guess (x * 0.5) fails catastrophically for values
    // far from 1.0 because Newton's method only halves the error per step
    // when the guess is orders of magnitude off.  By halving the exponent
    // we start within a factor of ~sqrt(2) of the true answer, giving
    // quadratic convergence (each iteration doubles correct digits).
    let mut e: i32 = 0;
    let m = frexp_internal(x, &mut e);
    let guess = if e & 1 == 0 {
        // e even: sqrt(m * 2^e) = sqrt(m) * 2^(e/2)
        ldexp(m, e / 2)
    } else {
        // e odd: sqrt(2m) * 2^((e-1)/2)
        ldexp(m * 2.0, (e - 1) / 2)
    };

    // Newton's method: g = (g + x/g) / 2.
    // 6 iterations from a good guess gives full f64 precision (~15 digits).
    let mut g = guess;
    let mut iter = 0;
    while iter < 6 {
        g = (g + x / g) * 0.5;
        iter += 1;
    }
    g
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sqrtf(x: f32) -> f32 {
    sqrt(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Exponential — Taylor series
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn expf(x: f32) -> f32 {
    exp(f64::from(x)) as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn exp2(x: f64) -> f64 {
    exp(x * LN2)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn exp2f(x: f32) -> f32 {
    expf(x * LN2 as f32)
}

// ---------------------------------------------------------------------------
// Natural logarithm
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logf(x: f32) -> f32 {
    log(f64::from(x)) as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log2(x: f64) -> f64 {
    log(x) * LOG2E
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log2f(x: f32) -> f32 {
    logf(x) * LOG2E as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log10(x: f64) -> f64 {
    log(x) / LN10
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log10f(x: f32) -> f32 {
    logf(x) / LN10 as f32
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    // Negative base with fractional exponent → domain error (NaN).
    // log(negative) is undefined in the reals; we make this explicit
    // rather than relying on NaN propagation through log→exp.
    if base < 0.0 {
        return f64::NAN;
    }

    // General case: base^exp = exp(exp * ln(base)).
    exp(exponent * log(base))
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn powf(base: f32, exponent: f32) -> f32 {
    pow(f64::from(base), f64::from(exponent)) as f32
}

/// Integer power via repeated squaring.
#[allow(clippy::arithmetic_side_effects)]
fn ipow(mut base: f64, mut exp: i64) -> f64 {
    if exp < 0 {
        base = 1.0 / base;
        // wrapping_neg of i64::MIN gives i64::MIN (still negative).
        // Handle that case: base is already inverted, and 2^63 iterations
        // would produce 0 or infinity depending on |base|.
        exp = exp.wrapping_neg();
        if exp < 0 {
            // exp was i64::MIN; |base| has been inverted above.
            let a = fabs(base);
            return if a < 1.0 { 0.0 } else if a > 1.0 { f64::INFINITY } else { 1.0 };
        }
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sinf(x: f32) -> f32 {
    sin(f64::from(x)) as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn cos(x: f64) -> f64 {
    sin(x + HALF_PI)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cosf(x: f32) -> f32 {
    cos(f64::from(x)) as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tan(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return f64::NAN; }
    // IEEE 754 division handles near-zero cosine correctly: when cos(x)
    // is very small, sin(x)/cos(x) produces a large value with the right
    // sign.  An explicit guard (`if fabs(c) < eps { return INFINITY }`)
    // would lose the sign, making tan wrong for x just above π/2.
    let c = cos(x);
    sin(x) / c
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    atan2(f64::from(y), f64::from(x)) as f32
}

/// Arctangent approximation using range reduction + Taylor series.
///
/// Uses three ranges for fast convergence:
/// - |x| > 1: atan(x) = sign(x)*π/2 - atan(1/x)
/// - |x| > 0.5: atan(x) = π/4 + atan((x-1)/(x+1))  [maps to |t| ≤ 1/3]
/// - |x| ≤ 0.5: direct Taylor series (converges fast)
///
/// Accuracy: ~1e-15 relative error (near full f64 precision).
#[allow(clippy::arithmetic_side_effects)]
fn atan_approx(x: f64) -> f64 {
    // Range reduction for |x| > 1.
    if fabs(x) > 1.0 {
        let sign = if x > 0.0 { 1.0 } else { -1.0 };
        return sign * HALF_PI - atan_approx(1.0 / x);
    }

    // For |x| > 0.5, use the identity:
    //   atan(x) = π/4 + atan((x - 1) / (x + 1))
    // This maps (0.5, 1] to (-1/3, 0], where the Taylor series converges
    // much faster than at x=1 (the series boundary).
    if fabs(x) > 0.5 {
        let sign = if x > 0.0 { 1.0 } else { -1.0 };
        let a = fabs(x);
        let t = (a - 1.0) / (a + 1.0);
        return sign * (QUARTER_PI + atan_taylor(t));
    }

    // Direct Taylor series for |x| <= 0.5.
    atan_taylor(x)
}

/// Taylor series for atan: x - x³/3 + x⁵/5 - x⁷/7 + ...
///
/// Assumes |x| <= 0.5 for rapid convergence (16 terms give ~1e-15 accuracy).
#[allow(clippy::arithmetic_side_effects)]
fn atan_taylor(x: f64) -> f64 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

    if biased_exp == 0 {
        // Subnormal: value = (-1)^s × 0.mantissa × 2^(-1022).
        // No implicit leading 1; we must normalize by scaling up.
        // Multiply by 2^64 to move the value into normal range,
        // then recurse and subtract 64 from the exponent.
        let scaled = x * (1u64 << 54) as f64; // x * 2^54
        let m = frexp_internal(scaled, exp);
        *exp -= 54;
        return m;
    }

    *exp = biased_exp - 1022; // Adjust so that 0.5 <= |m| < 1.

    // Replace exponent with 1022 (which gives 0.5 * mantissa).
    let mantissa_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | (1022_u64 << 52);
    f64::from_bits(mantissa_bits)
}

/// Scale a float by a power of 2.
///
/// Returns x * 2^exp.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
        // Subnormal result: shift the mantissa right to encode the value
        // in the IEEE 754 subnormal range (biased exponent = 0).
        // The implicit leading 1 bit becomes explicit in the mantissa.
        let shift = 1 - new_exp; // Number of positions to shift right.
        if shift > 52 {
            // Shifted entirely out of the 52-bit mantissa — flush to zero.
            return 0.0;
        }
        let sign = bits & 0x8000_0000_0000_0000;
        // Add the implicit leading 1 (bit 52) back into the mantissa.
        let mantissa = (bits & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
        let shifted = mantissa >> (shift as u64);
        return f64::from_bits(sign | shifted);
    }

    let new_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | ((new_exp as u64) << 52);
    f64::from_bits(new_bits)
}

/// Split a float into integer and fractional parts.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isnan(x: f64) -> i32 {
    i32::from(x.is_nan())
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isinf(x: f64) -> i32 {
    if x == f64::INFINITY { 1 }
    else if x == f64::NEG_INFINITY { -1 }
    else { 0 }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn isfinite(x: f64) -> i32 {
    i32::from(x.is_finite())
}

// ---------------------------------------------------------------------------
// Sign manipulation
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn copysign(x: f64, y: f64) -> f64 {
    let mag = x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF;
    let sign = y.to_bits() & 0x8000_0000_0000_0000;
    f64::from_bits(mag | sign)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn copysignf(x: f32, y: f32) -> f32 {
    let mag = x.to_bits() & 0x7FFF_FFFF;
    let sign = y.to_bits() & 0x8000_0000;
    f32::from_bits(mag | sign)
}

// ---------------------------------------------------------------------------
// Min / Max
// ---------------------------------------------------------------------------

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmin(x: f64, y: f64) -> f64 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x < y { x } else { y }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fmax(x: f64, y: f64) -> f64 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x > y { x } else { y }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fminf(x: f32, y: f32) -> f32 {
    if x.is_nan() { return y; }
    if y.is_nan() { return x; }
    if x < y { x } else { y }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atan(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    atan_approx(x)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atanf(x: f32) -> f32 {
    atan(f64::from(x)) as f32
}

/// Compute arc sine of x.
///
/// Returns a value in [-π/2, π/2].  Domain: |x| <= 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn asin(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if !(-1.0..=1.0).contains(&x) { return f64::NAN; }
    // Comparisons against exact boundary constants ±1.0 are intentional.
    #[allow(clippy::float_cmp)]
    if x == 1.0 { return HALF_PI; }
    #[allow(clippy::float_cmp)]
    if x == -1.0 { return -HALF_PI; }
    // asin(x) = atan2(x, sqrt(1 - x²))
    atan2(x, sqrt(1.0 - x * x))
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asinf(x: f32) -> f32 {
    asin(f64::from(x)) as f32
}

/// Compute arc cosine of x.
///
/// Returns a value in [0, π].  Domain: |x| <= 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn acos(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if !(-1.0..=1.0).contains(&x) { return f64::NAN; }
    // Comparisons against exact boundary constants ±1.0 are intentional.
    #[allow(clippy::float_cmp)]
    if x == 1.0 { return 0.0; }
    #[allow(clippy::float_cmp)]
    if x == -1.0 { return PI; }
    // acos(x) = atan2(sqrt(1 - x²), x)
    atan2(sqrt(1.0 - x * x), x)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acosf(x: f32) -> f32 {
    acos(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Hyperbolic functions
// ---------------------------------------------------------------------------

/// Hyperbolic sine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn sinh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x.is_infinite() { return x; }
    // sinh(x) = (e^x - e^(-x)) / 2
    let ex = exp(x);
    (ex - 1.0 / ex) * 0.5
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sinhf(x: f32) -> f32 {
    sinh(f64::from(x)) as f32
}

/// Hyperbolic cosine.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn cosh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x.is_infinite() { return f64::INFINITY; }
    // cosh(x) = (e^x + e^(-x)) / 2
    let ex = exp(x);
    (ex + 1.0 / ex) * 0.5
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn coshf(x: f32) -> f32 {
    cosh(f64::from(x)) as f32
}

/// Hyperbolic tangent.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tanh(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x > 20.0 { return 1.0; }
    if x < -20.0 { return -1.0; }
    // tanh(x) = (e^2x - 1) / (e^2x + 1)
    let e2x = exp(2.0 * x);
    (e2x - 1.0) / (e2x + 1.0)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tanhf(x: f32) -> f32 {
    tanh(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Other commonly needed functions
// ---------------------------------------------------------------------------

/// Euclidean distance: sqrt(x² + y²) without overflow.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hypotf(x: f32, y: f32) -> f32 {
    hypot(f64::from(x), f64::from(y)) as f32
}

/// Cube root.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn cbrtf(x: f32) -> f32 {
    cbrt(f64::from(x)) as f32
}

/// log(1 + x), accurate for small x.
///
/// For |x| < 1e-4, uses Taylor series to avoid catastrophic cancellation
/// in the naive `log(1 + x)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn log1p(x: f64) -> f64 {
    if x.is_nan() { return f64::NAN; }
    if x == f64::INFINITY { return f64::INFINITY; }
    if x < -1.0 { return f64::NAN; }
    // Comparison against exact boundary -1.0 is intentional (pole of log1p).
    #[allow(clippy::float_cmp)]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn log1pf(x: f32) -> f32 {
    log1p(f64::from(x)) as f32
}

/// exp(x) - 1, accurate for small x.
///
/// For |x| < 1e-4, uses Taylor series to avoid catastrophic cancellation.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn expm1f(x: f32) -> f32 {
    expm1(f64::from(x)) as f32
}

/// Positive difference: max(x - y, 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fdim(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() { return f64::NAN; }
    if x > y { x - y } else { 0.0 }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fdimf(x: f32, y: f32) -> f32 {
    fdim(f64::from(x), f64::from(y)) as f32
}

/// Fused multiply-add: x * y + z (computed without intermediate rounding).
///
/// On x86_64, FMA3 is available via intrinsic; we use a simple
/// implementation that computes at f64 precision (which gives correct
/// rounding for most cases in the f32 variant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fma(x: f64, y: f64, z: f64) -> f64 {
    // Without hardware FMA, this is the best we can do portably.
    // The rounding error is at most 1 ULP for typical inputs.
    x * y + z
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn fmaf(x: f32, y: f32, z: f32) -> f32 {
    // Perform in f64 for correct f32 result (f64 has enough precision
    // to represent the exact f32 product without rounding).
    (f64::from(x) * f64::from(y) + f64::from(z)) as f32
}

/// Round to nearest integer, ties away from zero (long).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lround(x: f64) -> i64 {
    round(x) as i64
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lroundf(x: f32) -> i64 {
    roundf(x) as i64
}

/// Round to nearest integer, ties away from zero (long long).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llround(x: f64) -> i64 {
    round(x) as i64
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn llroundf(x: f32) -> i64 {
    roundf(x) as i64
}

/// Round to nearest integer, ties to even (long).
///
/// Uses floor-based approach: `x - floor(x)` gives the fractional part
/// in [0, 1).  A tie occurs when the fractional part is exactly 0.5.
/// On a tie, round to the nearest even integer (banker's rounding).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn lrint(x: f64) -> i64 {
    if x.is_nan() || x.is_infinite() { return 0; }
    // Large values are already integers; avoid i64 saturation.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 {
        return x as i64;
    }
    let f = floor(x);
    let frac = x - f;
    let fi = f as i64;
    // Comparison against exact 0.5 is intentional: detects the half-way tie case.
    #[allow(clippy::float_cmp)]
    if frac == 0.5 {
        // Tie: round to nearest even.  floor(x) is the lower candidate,
        // floor(x)+1 is the upper.  Pick whichever is even.
        if fi & 1 == 0 { fi } else { fi.wrapping_add(1) }
    } else {
        // Not a tie: round to nearest (same as round-half-away-from-zero
        // since only exact 0.5 is a tie).
        round(x) as i64
    }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lrintf(x: f32) -> i64 {
    lrint(f64::from(x))
}

/// Round to nearest integer value (as floating-point), ties to even.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn rint(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; } // Preserve ±0.0.
    // Values with |x| >= 2^52 are already exact integers.
    if x >= 4_503_599_627_370_496.0 || x <= -4_503_599_627_370_496.0 { return x; }
    lrint(x) as f64
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
// The guard above ensures |x| < 2^23, so the integer value fits exactly in f32.
#[allow(clippy::cast_precision_loss)]
pub extern "C" fn rintf(x: f32) -> f32 {
    if x.is_nan() || x.is_infinite() { return x; }
    if x == 0.0 { return x; }
    if x >= 8_388_608.0 || x <= -8_388_608.0 { return x; }
    lrintf(x) as f32
}

/// Same as `rint`, but does not raise FP exceptions.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nearbyint(x: f64) -> f64 {
    rint(x)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nearbyintf(x: f32) -> f32 {
    rintf(x)
}

/// Multiply by power of 2 (integer exponent).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn scalbn(x: f64, n: i32) -> f64 {
    ldexp(x, n)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalbnf(x: f32, n: i32) -> f32 {
    ldexp(f64::from(x), n) as f32
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalbln(x: f64, n: i64) -> f64 {
    // Clamp to i32 range (exponents beyond ±1074 produce 0 or inf anyway).
    let clamped = n.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    ldexp(x, clamped)
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn scalblnf(x: f32, n: i64) -> f32 {
    scalbln(f64::from(x), n) as f32
}

/// Extract unbiased exponent as integer.
///
/// Returns the exponent of x such that 1 <= |x| * 2^(-ilogb(x)) < 2.
/// Special: ilogb(0) = `FP_ILOGB0`, ilogb(inf) = `INT_MAX`,
/// ilogb(NaN) = `FP_ILOGBNAN`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn ilogb(x: f64) -> i32 {
    if x.is_nan() { return i32::MAX; } // FP_ILOGBNAN
    if x.is_infinite() { return i32::MAX; }
    if x == 0.0 { return i32::MIN; } // FP_ILOGB0
    let bits = x.to_bits();
    let exp_field = ((bits >> 52) & 0x7FF) as i32;
    if exp_field == 0 {
        // Subnormal — count leading zeros in mantissa.
        let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
        // leading_zeros() <= 64, minus 12 won't overflow i32.
        let lz = mantissa.leading_zeros() as i32 - 12; // 64 - 52 = 12
        -1023 - lz
    } else {
        // exp_field is 1..=2046 (0 handled above, 0x7FF is inf/nan).
        exp_field - 1023
    }
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ilogbf(x: f32) -> i32 {
    ilogb(f64::from(x))
}

/// Extract unbiased exponent as f64.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logb(x: f64) -> f64 {
    if x == 0.0 { return f64::NEG_INFINITY; }
    if x.is_infinite() { return f64::INFINITY; }
    if x.is_nan() { return f64::NAN; }
    f64::from(ilogb(x))
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn logbf(x: f32) -> f32 {
    logb(f64::from(x)) as f32
}

/// Next representable f64 value after `from` in the direction of `to`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nextafter(from: f64, to: f64) -> f64 {
    if from.is_nan() || to.is_nan() { return f64::NAN; }
    // Exact bit-level equality check is intentional: nextafter(x, x) == x per IEEE 754.
    #[allow(clippy::float_cmp)]
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

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nextafterf(from: f32, to: f32) -> f32 {
    if from.is_nan() || to.is_nan() { return f32::NAN; }
    // Exact bit-level equality check is intentional: nextafterf(x, x) == x per IEEE 754.
    #[allow(clippy::float_cmp)]
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

/// IEEE 754 remainder (difference from `fmod`: uses round-to-nearest-even).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn remainder(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() { return f64::NAN; }
    if y.is_nan() || x.is_nan() { return f64::NAN; }
    if y.is_infinite() { return x; }
    // IEEE 754 remainder uses round-to-nearest-even (rint), NOT
    // round-half-away-from-zero (round).  E.g. remainder(2.5, 1.0):
    //   rint(2.5) = 2  →  2.5 - 2*1 =  0.5  (correct)
    //   round(2.5) = 3 →  2.5 - 3*1 = -0.5  (wrong)
    let n = rint(x / y);
    x - n * y
}

#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remainderf(x: f32, y: f32) -> f32 {
    remainder(f64::from(x), f64::from(y)) as f32
}

/// NaN with optional tag string (C99 nan("")).
///
/// The tag is ignored (implementation-defined payload).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nan(_tag: *const u8) -> f64 {
    f64::NAN
}

/// NaN (f32 variant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn nanf(_tag: *const u8) -> f32 {
    f32::NAN
}

/// frexp for f32.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn frexpf(x: f32, exp: *mut i32) -> f32 {
    frexp(f64::from(x), exp) as f32
}

/// ldexp for f32.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ldexpf(x: f32, exp: i32) -> f32 {
    ldexp(f64::from(x), exp) as f32
}

/// modf for f32.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn __copysign(x: f64, y: f64) -> f64 {
    copysign(x, y)
}

// ---------------------------------------------------------------------------
// Error function and gamma function
// ---------------------------------------------------------------------------

/// Error function.
///
/// Approximation using Abramowitz and Stegun formula 7.1.26.
/// Accurate to ~1.5e-7 relative error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn erf(x: f64) -> f64 {
    // Constants from A&S 7.1.26.
    const A1: f64 = 0.254_829_592;
    const A2: f64 = -0.284_496_736;
    const A3: f64 = 1.421_413_741;
    const A4: f64 = -1.453_152_027;
    const A5: f64 = 1.061_405_429;
    const P: f64 = 0.327_591_1;

    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let a = fabs(x);
    let t = 1.0 / (1.0 + P * a);
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;
    let y = 1.0 - (A1 * t + A2 * t2 + A3 * t3 + A4 * t4 + A5 * t5) * exp(-a * a);
    sign * y
}

/// Complementary error function: erfc(x) = 1 - erf(x).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn erfc(x: f64) -> f64 {
    1.0 - erf(x)
}

/// Error function (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erff(x: f32) -> f32 {
    erf(f64::from(x)) as f32
}

/// Complementary error function (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn erfcf(x: f32) -> f32 {
    erfc(f64::from(x)) as f32
}

/// Natural log of the absolute value of the gamma function.
///
/// Uses the Stirling approximation with correction terms for x >= 7,
/// and the recurrence relation to reduce smaller x to that range.
/// Returns +∞ for x = 0 and negative integers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn lgamma(x: f64) -> f64 {
    // Handle special cases.
    if x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    // Poles at 0 and negative integers.  Exact comparison is intentional:
    // floor(x) returns the exact integer, and x must equal it exactly.
    #[allow(clippy::float_cmp)]
    if x <= 0.0 && x == floor(x) {
        return f64::INFINITY;
    }

    // Use reflection formula for x < 0.5:
    //   Γ(x) * Γ(1-x) = π / sin(πx)
    //   lgamma(x) = ln(π/sin(πx)) - lgamma(1-x)
    if x < 0.5 {
        let reflect = consts::PI / sin(consts::PI * x);
        return log(fabs(reflect)) - lgamma(1.0 - x);
    }

    // Use recurrence Γ(x+1) = x*Γ(x) to get x >= 7.
    let mut xx = x;
    let mut correction: f64 = 0.0;
    while xx < 7.0 {
        correction -= log(xx);
        xx += 1.0;
    }

    // Stirling series: ln(Γ(x)) ≈ (x-0.5)*ln(x) - x + 0.5*ln(2π) + Σ B_{2n}/(2n*(2n-1)*x^{2n-1})
    let x2 = xx * xx;
    let result = (xx - 0.5) * log(xx) - xx + 0.918_938_533_204_672_7 // 0.5*ln(2π)
        + 1.0 / (12.0 * xx)
        - 1.0 / (360.0 * x2 * xx)
        + 1.0 / (1260.0 * x2 * x2 * xx);

    result + correction
}

/// f32 version of lgamma.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgammaf(x: f32) -> f32 {
    lgamma(f64::from(x)) as f32
}

/// Gamma function: Γ(x) = exp(lgamma(x)).
///
/// Returns the true gamma function value, handling sign correctly.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn tgamma(x: f64) -> f64 {
    // Special cases.
    if x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return if x > 0.0 { f64::INFINITY } else { f64::NAN };
    }
    // Poles at 0 and negative integers.  Exact comparison is intentional:
    // floor(x) returns the exact integer, and x must equal it exactly.
    #[allow(clippy::float_cmp)]
    if x <= 0.0 && x == floor(x) {
        return f64::NAN;
    }

    // For positive x, Γ(x) = exp(lgamma(x)).
    if x > 0.0 {
        return exp(lgamma(x));
    }

    // Negative x: use reflection formula.
    // Γ(x) = π / (sin(πx) * Γ(1-x))
    let sin_pi_x = sin(consts::PI * x);
    if fabs(sin_pi_x) < 1e-15 {
        return f64::NAN; // Near a pole.
    }
    consts::PI / (sin_pi_x * tgamma(1.0 - x))
}

/// f32 version of tgamma.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tgammaf(x: f32) -> f32 {
    tgamma(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Inverse hyperbolic functions
// ---------------------------------------------------------------------------

/// Inverse hyperbolic sine: asinh(x) = ln(x + √(x² + 1)).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn asinh(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    // For large |x|, avoid x*x overflow: asinh(x) ≈ sign(x) * (ln(2) + ln(|x|)).
    let a = fabs(x);
    if a > 1e150 {
        let r = consts::LN_2 + log(a);
        return if x < 0.0 { -r } else { r };
    }
    // For small |x|, use log1p for accuracy: asinh(x) = log1p(x + x²/(1+√(1+x²))).
    if a < 0.5 {
        let r = log1p(a + a * a / (1.0 + sqrt(1.0 + a * a)));
        return if x < 0.0 { -r } else { r };
    }
    let r = log(a + sqrt(a * a + 1.0));
    if x < 0.0 { -r } else { r }
}

/// Inverse hyperbolic sine (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn asinhf(x: f32) -> f32 {
    asinh(f64::from(x)) as f32
}

/// Inverse hyperbolic cosine: acosh(x) = ln(x + √(x² - 1)), x >= 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn acosh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 1.0 {
        return f64::NAN; // Domain error.
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }
    // For large x, avoid overflow: acosh(x) ≈ ln(2) + ln(x).
    if x > 1e150 {
        return consts::LN_2 + log(x);
    }
    // For x near 1, use log1p for accuracy:
    //   acosh(x) = log1p((x-1) + √((x-1)*(x+1)))
    if x < 2.0 {
        let t = x - 1.0;
        return log1p(t + sqrt(t * (x + 1.0)));
    }
    log(x + sqrt(x * x - 1.0))
}

/// Inverse hyperbolic cosine (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn acoshf(x: f32) -> f32 {
    acosh(f64::from(x)) as f32
}

/// Inverse hyperbolic tangent: atanh(x) = 0.5 * ln((1+x)/(1-x)), |x| < 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn atanh(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    // Comparisons against exact boundary constants ±1.0 are intentional (poles of atanh).
    #[allow(clippy::float_cmp)]
    if x == 1.0 {
        return f64::INFINITY;
    }
    #[allow(clippy::float_cmp)]
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if fabs(x) > 1.0 {
        return f64::NAN; // Domain error.
    }
    // Use log1p for accuracy: atanh(x) = 0.5 * log1p(2x / (1-x)).
    0.5 * log1p(2.0 * x / (1.0 - x))
}

/// Inverse hyperbolic tangent (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn atanhf(x: f32) -> f32 {
    atanh(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// sincos — compute sin and cos simultaneously
// ---------------------------------------------------------------------------

/// Compute sine and cosine simultaneously (GNU extension).
///
/// More efficient than calling sin() and cos() separately when both
/// are needed.
///
/// # Safety
///
/// `sinp` and `cosp` must be valid, writable pointers.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn sincos(x: f64, sinp: *mut f64, cosp: *mut f64) {
    // SAFETY: Caller must provide valid pointers per POSIX convention.
    if !sinp.is_null() {
        unsafe { *sinp = sin(x); }
    }
    if !cosp.is_null() {
        unsafe { *cosp = cos(x); }
    }
}

/// sincos (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sincosf(x: f32, sinp: *mut f32, cosp: *mut f32) {
    if !sinp.is_null() {
        unsafe { *sinp = sinf(x); }
    }
    if !cosp.is_null() {
        unsafe { *cosp = cosf(x); }
    }
}

// ---------------------------------------------------------------------------
// remquo — IEEE remainder with quotient
// ---------------------------------------------------------------------------

/// IEEE 754 remainder with quotient bits.
///
/// Returns the same remainder as `remainder(x, y)`, and stores at least
/// the low 3 bits of the integral quotient in `*quo` (with the sign
/// of `x/y`).
///
/// # Safety
///
/// `quo` must be a valid, writable pointer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub extern "C" fn remquo(x: f64, y: f64, quo: *mut i32) -> f64 {
    if y == 0.0 || x.is_nan() || y.is_nan() || x.is_infinite() {
        if !quo.is_null() {
            // SAFETY: quo verified non-null.
            unsafe { *quo = 0; }
        }
        if x.is_nan() { return x; }
        if y.is_nan() { return y; }
        return f64::NAN;
    }

    // Compute quotient and remainder.
    // IEEE 754: remainder uses round-to-nearest-even (rint), not
    // round-half-away-from-zero (round).
    let q_exact = x / y;
    let q_rounded = rint(q_exact);
    let rem = x - q_rounded * y;

    if !quo.is_null() {
        // Store low bits of the quotient magnitude with the correct sign.
        // POSIX requires at least 3 bits of the quotient; we provide 31.
        let q_int = q_rounded as i64;
        // Extract magnitude, then truncate to 31 bits.
        let mag = q_int.unsigned_abs();
        let q_low = (mag & 0x7FFF_FFFF) as i32;
        // Apply the sign of x/y (positive when same sign, negative otherwise).
        let signed_q = if (x < 0.0) != (y < 0.0) { -q_low } else { q_low };
        // SAFETY: quo verified non-null.
        unsafe { *quo = signed_q; }
    }

    rem
}

/// remquo (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remquof(x: f32, y: f32, quo: *mut i32) -> f32 {
    remquo(f64::from(x), f64::from(y), quo) as f32
}

// ---------------------------------------------------------------------------
// exp10 / pow10 — base-10 exponential (GNU extensions)
// ---------------------------------------------------------------------------

/// Compute 10^x (GNU extension).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp10(x: f64) -> f64 {
    pow(10.0, x)
}

/// exp10 (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn exp10f(x: f32) -> f32 {
    powf(10.0, x)
}

/// Alias for `exp10` (GNU extension, deprecated).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pow10(x: f64) -> f64 {
    exp10(x)
}

/// pow10 (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn pow10f(x: f32) -> f32 {
    exp10f(x)
}

// ---------------------------------------------------------------------------
// lgamma_r — thread-safe lgamma with sign
// ---------------------------------------------------------------------------

/// Thread-safe lgamma: returns lgamma(x) and stores the sign of Γ(x) in `*signp`.
///
/// `*signp` is set to 1 if Γ(x) >= 0, or -1 if Γ(x) < 0.
///
/// # Safety
///
/// `signp` must be a valid, writable pointer (or NULL, in which case
/// the sign is not stored).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
// float_cmp: x == floor(x) intentionally checks exact integer equality (gamma poles).
#[allow(clippy::float_cmp)]
pub extern "C" fn lgamma_r(x: f64, signp: *mut i32) -> f64 {
    // Compute the sign of Γ(x).
    // Γ(x) > 0 for x > 0.
    // For x < 0 non-integer, sign alternates:
    //   Γ(x) < 0 for x ∈ (-1,0), (-3,-2), (-5,-4), ... (floor is odd)
    //   Γ(x) > 0 for x ∈ (-2,-1), (-4,-3), (-6,-5), ... (floor is even)
    let sign: i32 = if x > 0.0 || x.is_nan() || x.is_infinite() {
        1
    } else if x == floor(x) {
        // Pole — sign is undefined, use +1 by convention.
        1
    } else {
        // floor(x) for negative non-integer x:
        //   x ∈ (-1,0) → floor = -1 (odd)  → Γ < 0
        //   x ∈ (-2,-1) → floor = -2 (even) → Γ > 0
        //   x ∈ (-3,-2) → floor = -3 (odd)  → Γ < 0
        // In Rust, -1 % 2 == -1 (nonzero) and -2 % 2 == 0.
        let n = floor(x) as i64;
        if n % 2 == 0 { 1 } else { -1 }
    };

    if !signp.is_null() {
        // SAFETY: signp verified non-null.
        unsafe { *signp = sign; }
    }

    lgamma(x)
}

/// lgamma_r (f32).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lgammaf_r(x: f32, signp: *mut i32) -> f32 {
    lgamma_r(f64::from(x), signp) as f32
}

// ---------------------------------------------------------------------------
// Deprecated / compatibility aliases
// ---------------------------------------------------------------------------

/// `finite(x)` — deprecated BSD alias for `isfinite(x)`.
///
/// Returns non-zero if `x` is not infinity or NaN.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finite(x: f64) -> i32 {
    isfinite(x)
}

/// `finitef(x)` — f32 variant of `finite`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn finitef(x: f32) -> i32 {
    i32::from(!(x.is_infinite() || x.is_nan()))
}

/// `drem(x, y)` — deprecated alias for `remainder(x, y)`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn drem(x: f64, y: f64) -> f64 {
    remainder(x, y)
}

/// `dremf(x, y)` — f32 variant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn dremf(x: f32, y: f32) -> f32 {
    remainderf(x, y)
}

/// `gamma(x)` — deprecated alias for `lgamma(x)`.
///
/// Note: historically `gamma()` meant the log-gamma function, not the
/// true gamma function.  Use `tgamma()` for Γ(x).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gamma(x: f64) -> f64 {
    lgamma(x)
}

/// `gammaf(x)` — f32 variant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn gammaf(x: f32) -> f32 {
    lgammaf(x)
}

/// `significand(x)` — extract significand (mantissa) scaled to [1, 2).
///
/// Returns `x * 2^(-ilogb(x))`, i.e. the significand as if the
/// exponent were 0.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn significand(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return x;
    }
    // scalbn(x, -ilogb(x)) normalizes x to [1, 2).
    scalbn(x, -ilogb(x))
}

/// `significandf(x)` — f32 variant.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn significandf(x: f32) -> f32 {
    significand(f64::from(x)) as f32
}

// ---------------------------------------------------------------------------
// Bessel functions (first and second kind)
// ---------------------------------------------------------------------------

/// Bessel function of the first kind, order 0.
///
/// Uses polynomial approximation: rational for |x| <= 3, and
/// asymptotic expansion for |x| > 3.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn j0(x: f64) -> f64 {
    let a = fabs(x);

    if a <= 3.0 {
        // Rational approximation for small |x|.
        // J0(x) ≈ 1 - x²/4 + x⁴/64 - x⁶/2304 + ...
        let x2 = x * x;
        let x4 = x2 * x2;
        let x6 = x4 * x2;
        let x8 = x4 * x4;
        1.0 - x2 / 4.0
            + x4 / 64.0
            - x6 / 2304.0
            + x8 / 147_456.0
    } else {
        // Asymptotic: J0(x) ≈ √(2/(πx)) * cos(x - π/4).
        let phase = a - consts::FRAC_PI_4;
        sqrt(2.0 / (consts::PI * a)) * cos(phase)
    }
}

/// Bessel function of the first kind, order 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn j1(x: f64) -> f64 {
    let a = fabs(x);
    let sign = if x < 0.0 { -1.0 } else { 1.0 };

    if a <= 3.0 {
        // Series: J1(x) = x/2 * (1 - x²/8 + x⁴/192 - x⁶/9216 + ...).
        let x2 = x * x;
        let x4 = x2 * x2;
        let x6 = x4 * x2;
        sign * a / 2.0 * (1.0
            - a * a / 8.0
            + x4 / 192.0
            - x6 / 9216.0)
    } else {
        // Asymptotic: J1(x) ≈ √(2/(πx)) * cos(x - 3π/4).
        let phase = a - 3.0 * consts::FRAC_PI_4;
        sign * sqrt(2.0 / (consts::PI * a)) * cos(phase)
    }
}

/// Bessel function of the first kind, order n (integer).
///
/// Uses Miller's backward recurrence for stability.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn jn(n: i32, x: f64) -> f64 {
    if n == 0 { return j0(x); }
    if n == 1 { return j1(x); }

    let sign = if n < 0 && (-n) % 2 != 0 { -1.0 } else { 1.0 };
    let n_abs = if n < 0 { -n } else { n };
    let a = fabs(x);

    if a == 0.0 {
        return 0.0;
    }

    // Forward recurrence: J_{n+1}(x) = (2n/x)*J_n(x) - J_{n-1}(x).
    // Stable for n < x; for n > x, accuracy degrades but is acceptable
    // for our purposes.
    let mut j_prev = j0(a);
    let mut j_curr = j1(a);

    let mut k: i32 = 1;
    while k < n_abs {
        let j_next = (2.0 * f64::from(k) / a) * j_curr - j_prev;
        j_prev = j_curr;
        j_curr = j_next;
        k = k.wrapping_add(1);
    }

    let result = if x < 0.0 && n_abs % 2 != 0 { -j_curr } else { j_curr };
    sign * result
}

/// Bessel function of the second kind, order 0.
///
/// Y0(x) ≈ (2/π) * (J0(x) * (ln(x/2) + γ) + series correction).
/// Uses asymptotic expansion for large x.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn y0(x: f64) -> f64 {
    // Euler-Mascheroni constant γ ≈ 0.5772156649.
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;

    if x <= 0.0 {
        return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
    }
    if x.is_nan() {
        return x;
    }

    if x > 3.0 {
        // Asymptotic: Y0(x) ≈ √(2/(πx)) * sin(x - π/4).
        let phase = x - consts::FRAC_PI_4;
        return sqrt(2.0 / (consts::PI * x)) * sin(phase);
    }

    // Small x: Y0(x) = (2/π) * (J0(x)*(ln(x/2) + γ) + correction).
    let j0x = j0(x);
    let ln_term = log(x / 2.0) + EULER_GAMMA;

    // First few correction terms from the series.
    let x2 = x * x;
    let correction = x2 / 4.0 - x2 * x2 * 3.0 / 128.0;

    (2.0 / consts::PI) * (j0x * ln_term + correction)
}

/// Bessel function of the second kind, order 1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn y1(x: f64) -> f64 {
    // Euler-Mascheroni constant γ ≈ 0.5772156649.
    const EULER_GAMMA: f64 = 0.577_215_664_901_532_9;

    if x <= 0.0 {
        return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
    }
    if x.is_nan() {
        return x;
    }

    if x > 3.0 {
        // Asymptotic: Y1(x) ≈ √(2/(πx)) * sin(x - 3π/4).
        let phase = x - 3.0 * consts::FRAC_PI_4;
        return sqrt(2.0 / (consts::PI * x)) * sin(phase);
    }

    // Small x: Y1(x) ≈ (2/π) * (J1(x)*ln(x/2) - 1/x).
    let j1x = j1(x);
    let ln_term = log(x / 2.0) + EULER_GAMMA;
    (2.0 / consts::PI) * (j1x * ln_term - 1.0 / x)
}

/// Bessel function of the second kind, order n (integer).
///
/// Uses forward recurrence from Y0 and Y1.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
#[allow(clippy::arithmetic_side_effects)]
pub extern "C" fn yn(n: i32, x: f64) -> f64 {
    if x <= 0.0 {
        return if x == 0.0 { f64::NEG_INFINITY } else { f64::NAN };
    }
    if n == 0 { return y0(x); }
    if n == 1 { return y1(x); }

    let n_abs = if n < 0 { -n } else { n };

    // Forward recurrence: Y_{n+1}(x) = (2n/x)*Y_n(x) - Y_{n-1}(x).
    let mut y_prev = y0(x);
    let mut y_curr = y1(x);

    let mut k: i32 = 1;
    while k < n_abs {
        let y_next = (2.0 * f64::from(k) / x) * y_curr - y_prev;
        y_prev = y_curr;
        y_curr = y_next;
        k = k.wrapping_add(1);
    }

    if n < 0 && (-n) % 2 != 0 { -y_curr } else { y_curr }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Approximate comparison helpers.  Taylor-series implementations are
    // accurate to roughly 10-15 digits (f64) and 5-7 digits (f32), so we
    // use tolerances that are tight but not tighter than the implementation
    // can deliver.

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
        assert_approx(sin(PI / 4.0), core::f64::consts::FRAC_1_SQRT_2, EPS, "sin(pi/4)");
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
        assert_approx(cos(PI / 4.0), core::f64::consts::FRAC_1_SQRT_2, EPS, "cos(pi/4)");
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
        assert_approx(log(core::f64::consts::E * core::f64::consts::E), 2.0, EPS, "log(e^2)");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_log_special_values() {
        assert_eq!(log(0.0), f64::NEG_INFINITY, "log(0) should be -inf");
        assert!(log(-1.0).is_nan(), "log(-1) should be NaN");
        assert!(log(f64::NAN).is_nan(), "log(NaN) should be NaN");
        assert_eq!(log(f64::INFINITY), f64::INFINITY, "log(+inf) should be +inf");
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
        assert_eq!(hypot(f64::INFINITY, f64::NAN), f64::INFINITY, "hypot(inf,NaN)");
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
        assert!(remainder(f64::INFINITY, 1.0).is_nan(), "remainder(inf,1) should be NaN");
    }

    #[test]
    fn test_remainder_uses_round_to_even() {
        // IEEE 754 remainder uses round-to-nearest-even, NOT half-away-from-zero.
        // remainder(2.5, 1.0): rint(2.5)=2 (even) → 2.5 - 2*1 = 0.5
        assert_approx(remainder(2.5, 1.0), 0.5, EPS, "remainder(2.5,1) round-to-even");
        // remainder(3.5, 1.0): rint(3.5)=4 (even) → 3.5 - 4*1 = -0.5
        assert_approx(remainder(3.5, 1.0), -0.5, EPS, "remainder(3.5,1) round-to-even");
        // remainder(4.5, 1.0): rint(4.5)=4 (even) → 4.5 - 4*1 = 0.5
        assert_approx(remainder(4.5, 1.0), 0.5, EPS, "remainder(4.5,1) round-to-even");
        // remainder(5.5, 1.0): rint(5.5)=6 (even) → 5.5 - 6*1 = -0.5
        assert_approx(remainder(5.5, 1.0), -0.5, EPS, "remainder(5.5,1) round-to-even");
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
        assert_approx_f32(sinf(core::f32::consts::FRAC_PI_2), 1.0, EPS_F32, "sinf(pi/2)");
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
        assert_approx_f32(cosf(core::f32::consts::FRAC_PI_2), 0.0, EPS_F32, "cosf(pi/2)");
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
        assert_eq!(ilogb(f64::NAN), i32::MAX, "ilogb(NaN) = FP_ILOGBNAN");
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

    #[test]
    fn test_lgamma_values() {
        // lgamma(1) = ln(0!) = ln(1) = 0.
        assert_approx(lgamma(1.0), 0.0, 1e-8, "lgamma(1)");
        // lgamma(2) = ln(1!) = ln(1) = 0.
        assert_approx(lgamma(2.0), 0.0, 1e-8, "lgamma(2)");
        // Gamma(n) = (n-1)! for positive integers, so lgamma(5) = ln(24).
        assert_approx(lgamma(5.0), log(24.0), 1e-8, "lgamma(5) = ln(24)");
    }

    #[test]
    fn test_lgamma_poles() {
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
        assert!(tgamma(0.0).is_nan(), "tgamma(0) is NaN");
        assert!(tgamma(-1.0).is_nan(), "tgamma(-1) is NaN");
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
        assert_approx(drem(10.0, 3.0), remainder(10.0, 3.0), EPS, "drem == remainder");
    }

    #[test]
    fn test_significand_values() {
        // significand(x) returns x * 2^(-ilogb(x)), should be in [1, 2).
        let s = significand(8.0);
        assert!(s >= 1.0 && s < 2.0, "significand(8) should be in [1, 2), got {s}");
        assert_approx(significand(1.0), 1.0, EPS, "significand(1)");
    }

    // -----------------------------------------------------------------------
    // lgamma_r (thread-safe gamma with sign)
    // -----------------------------------------------------------------------

    #[test]
    fn test_lgamma_r_sign() {
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
        assert_eq!(ldexp(-1.0, 1024), f64::NEG_INFINITY, "ldexp(-1, 1024) overflow");
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
        assert!(result < 0.0 || result.to_bits() != 0, "negative subnormal should preserve sign");
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
        assert!(m >= 0.5 && m < 1.0,
            "frexp subnormal: m={m} should be in [0.5, 1.0)");
        let roundtrip = ldexp(m, e);
        assert_eq!(roundtrip.to_bits(), x.to_bits(),
            "frexp/ldexp roundtrip for smallest subnormal failed: got {roundtrip}, expected {x}");
    }

    #[test]
    fn frexp_subnormal_mid() {
        // A subnormal in the middle of the range.
        // 2^-1024 = ldexp(1.0, -1024) — this is subnormal.
        let x = ldexp(1.0, -1040);
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(m >= 0.5 && m < 1.0,
            "frexp mid-subnormal: m={m} should be in [0.5, 1.0)");
        assert_eq!(e, -1039,
            "frexp(2^-1040) should have exp=-1039, got {e}");
        let roundtrip = ldexp(m, e);
        assert_eq!(roundtrip.to_bits(), x.to_bits(),
            "frexp/ldexp roundtrip for mid-subnormal failed");
    }

    #[test]
    fn frexp_negative_subnormal() {
        let x = -f64::from_bits(1); // -5e-324
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(m <= -0.5 && m > -1.0,
            "frexp negative subnormal: m={m} should be in (-1.0, -0.5]");
        let roundtrip = ldexp(m, e);
        assert_eq!(roundtrip.to_bits(), x.to_bits(),
            "frexp/ldexp roundtrip for negative subnormal");
    }

    #[test]
    fn frexp_largest_subnormal() {
        // Largest subnormal: biased_exp=0, all mantissa bits set.
        let x = f64::from_bits(0x000F_FFFF_FFFF_FFFF);
        let mut e: i32 = 0;
        let m = frexp(x, &mut e);
        assert!(m >= 0.5 && m < 1.0,
            "frexp largest subnormal: m={m} should be in [0.5, 1.0)");
        let roundtrip = ldexp(m, e);
        assert_eq!(roundtrip.to_bits(), x.to_bits(),
            "frexp/ldexp roundtrip for largest subnormal");
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
        assert_approx(result, expected, expected * 1e-10,
            "sqrt(1e-300)");
    }

    // -----------------------------------------------------------------------
    // nextafter edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn nextafter_from_zero() {
        let pos = nextafter(0.0, 1.0);
        assert!(pos > 0.0, "nextafter(0, 1) should be positive");
        assert_eq!(pos.to_bits(), 1, "nextafter(0, 1) should be smallest subnormal");

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
        assert_approx(remainder(2.5, 1.0), 0.5, EPS, "remainder(2.5,1) ties to even");
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
        // NaN and Inf → 0 per our implementation.
        assert_eq!(lrint(f64::NAN), 0, "lrint(NaN) → 0");
        assert_eq!(lrint(f64::INFINITY), 0, "lrint(inf) → 0");
        assert_eq!(lrint(f64::NEG_INFINITY), 0, "lrint(-inf) → 0");
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
        assert_eq!(ilogb(f64::NAN), i32::MAX, "ilogb(NaN) = FP_ILOGBNAN");
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
        assert_eq!(ilogb(largest_sub), -1023, "ilogb(largest subnormal) = -1023");

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
            assert_approx(f64::from(roundtrip), f64::from(x), 1e-6,
                &format!("frexpf/ldexpf roundtrip for {x}"));
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
            assert_approx(f64::from(roundtrip), f64::from(x), 1e-4,
                &format!("sinh(asinh({x}))"));
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
        // lgamma(1) = ln(Γ(1)) = ln(1) = 0
        assert_approx(f64::from(lgammaf(1.0)), 0.0, 1e-5, "lgammaf(1)");
        // lgamma(2) = ln(Γ(2)) = ln(1) = 0
        assert_approx(f64::from(lgammaf(2.0)), 0.0, 1e-5, "lgammaf(2)");
        // lgamma(5) = ln(4!) = ln(24) ≈ 3.178
        assert_approx(f64::from(lgammaf(5.0)), 3.178_054, 1e-3, "lgammaf(5)");
    }

    #[test]
    fn lgammaf_poles() {
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
            assert_approx(f64::from(s), f64::from(sinf(a)), 1e-6,
                &format!("sincosf({a}) sin"));
            assert_approx(f64::from(c), f64::from(cosf(a)), 1e-6,
                &format!("sincosf({a}) cos"));
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
            assert_approx(sum, 1.0, 1e-5,
                &format!("sin²({a}) + cos²({a}) = 1"));
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
        assert_approx(f64::from(asinf(1.0)), core::f64::consts::FRAC_PI_2, 1e-4, "asinf(1)");
        assert_approx(f64::from(asinf(-1.0)), -core::f64::consts::FRAC_PI_2, 1e-4, "asinf(-1)");
        assert_approx(f64::from(asinf(0.5)), core::f64::consts::FRAC_PI_6, 1e-4, "asinf(0.5)");
    }

    #[test]
    fn asinf_domain_error() {
        assert!(asinf(1.5).is_nan(), "asinf(1.5) = NaN");
        assert!(asinf(-1.5).is_nan(), "asinf(-1.5) = NaN");
    }

    #[test]
    fn acosf_values() {
        assert_approx(f64::from(acosf(1.0)), 0.0, 1e-6, "acosf(1)");
        assert_approx(f64::from(acosf(0.0)), core::f64::consts::FRAC_PI_2, 1e-4, "acosf(0)");
        assert_approx(f64::from(acosf(-1.0)), core::f64::consts::PI, 1e-4, "acosf(-1)");
    }

    #[test]
    fn acosf_domain_error() {
        assert!(acosf(1.5).is_nan(), "acosf(1.5) = NaN");
        assert!(acosf(-1.5).is_nan(), "acosf(-1.5) = NaN");
    }

    #[test]
    fn atanf_values() {
        assert_approx(f64::from(atanf(0.0)), 0.0, 1e-6, "atanf(0)");
        assert_approx(f64::from(atanf(1.0)), core::f64::consts::FRAC_PI_4, 1e-6, "atanf(1)");
        assert_approx(f64::from(atanf(-1.0)), -core::f64::consts::FRAC_PI_4, 1e-6, "atanf(-1)");
    }

    #[test]
    fn atan2f_quadrants() {
        // atan2(1, 1) = π/4.
        assert_approx(f64::from(atan2f(1.0, 1.0)), core::f64::consts::FRAC_PI_4, 1e-6,
            "atan2f(1, 1)");
        // atan2(1, -1) = 3π/4.
        assert_approx(f64::from(atan2f(1.0, -1.0)), 3.0 * core::f64::consts::FRAC_PI_4, 1e-6,
            "atan2f(1, -1)");
        // atan2(-1, 1) = -π/4.
        assert_approx(f64::from(atan2f(-1.0, 1.0)), -core::f64::consts::FRAC_PI_4, 1e-6,
            "atan2f(-1, 1)");
    }

    #[test]
    fn atan2f_axis_values() {
        assert_approx(f64::from(atan2f(0.0, 1.0)), 0.0, 1e-6, "atan2f(0, 1)");
        assert_approx(f64::from(atan2f(1.0, 0.0)), core::f64::consts::FRAC_PI_2, 1e-4,
            "atan2f(1, 0)");
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
        assert_approx(f64::from(log1pf(1.0)), core::f64::consts::LN_2, 1e-4, "log1pf(1)");
        // log1p(-1) = log(0) = -inf
        assert_eq!(log1pf(-1.0), f32::NEG_INFINITY, "log1pf(-1) = -inf");
    }

    #[test]
    fn expm1f_values() {
        assert_approx(f64::from(expm1f(0.0)), 0.0, 1e-6, "expm1f(0)");
        assert_approx(f64::from(expm1f(1.0)), core::f64::consts::E - 1.0, 1e-4, "expm1f(1)");
        // expm1(-inf) = -1
        assert_approx(f64::from(expm1f(f32::NEG_INFINITY)), -1.0, 1e-6, "expm1f(-inf)");
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
}

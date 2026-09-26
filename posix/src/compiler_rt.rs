//! The compiler-rt builtins that Rust's `compiler_builtins` implements only in C.
//!
//! A C compiler lowers some operations to calls into a runtime library rather
//! than to instructions: `_Complex` multiplication and division, `-ftrapv`'s
//! overflow-checked arithmetic, and a few bit operations and comparisons on
//! targets or types it will not inline. LLVM's library for those is
//! compiler-rt; Rust's is `compiler_builtins`, which implements most of
//! compiler-rt in Rust but these only in C, compiled when its `c` feature is on.
//!
//! The precompiled standard library for `x86_64-unknown-none` has that feature,
//! so the old `libc.a`, which carried it, supplied them. A `libc.a` built with
//! `-Zbuild-std` for its own hard-float target does not: the `c` feature needs
//! compiler-rt's C sources, which `rust-src` does not ship
//! (known-issues.md `TD-D-THE-SYSROOT-FIX-RESTS-ON-A-FLAG-RUSTC-IS-PHASING-OUT`).
//! This module supplies the same set. The list is exactly what the old archive
//! defined that the new one did not, measured with `nm`. CMake's binary uses
//! some of them.
//!
//! Each is a port of the compiler-rt function of the same name (Apache-2.0 WITH
//! LLVM-exception); the complex ones follow C99 Annex G, as compiler-rt does,
//! so that an infinite operand gives an infinite result rather than NaN.
//!
//! On the host they are ordinary Rust functions: the host's own runtime
//! already exports these names, and a second definition would clash.

use core::ffi::c_char;

/// A C `float _Complex`: returned in `%xmm0`, both halves packed, which is
/// what the SysV ABI gives a two-`float` struct as well.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex32 {
    pub re: f32,
    pub im: f32,
}

/// A C `double _Complex`: returned in `%xmm0` and `%xmm1`, as a two-`double`
/// struct is.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex64 {
    pub re: f64,
    pub im: f64,
}

/// compiler-rt's `compilerrt_abort`: an operation `-ftrapv` promised would
/// not overflow did.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __compilerrt_abort_impl(
    _file: *const c_char,
    _line: i32,
    _function: *const c_char,
) -> ! {
    crate::unistd::abort()
}

fn trap() -> ! {
    __compilerrt_abort_impl(core::ptr::null(), 0, core::ptr::null())
}

// -- -ftrapv: arithmetic that aborts on overflow ---------------------------------

macro_rules! trapping {
    ($(($abs:ident, $neg:ident, $add:ident, $sub:ident, $mul:ident, $t:ty)),* $(,)?) => {$(
        #[doc = concat!("`-ftrapv`'s `abs` for `", stringify!($t), "`: aborts on the minimum value.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $abs(a: $t) -> $t {
            match a.checked_abs() {
                Some(v) => v,
                None => trap(),
            }
        }

        #[doc = concat!("`-ftrapv`'s negation for `", stringify!($t), "`: aborts on the minimum value.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $neg(a: $t) -> $t {
            match a.checked_neg() {
                Some(v) => v,
                None => trap(),
            }
        }

        #[doc = concat!("`-ftrapv`'s addition for `", stringify!($t), "`: aborts on overflow.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $add(a: $t, b: $t) -> $t {
            match a.checked_add(b) {
                Some(v) => v,
                None => trap(),
            }
        }

        #[doc = concat!("`-ftrapv`'s subtraction for `", stringify!($t), "`: aborts on overflow.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $sub(a: $t, b: $t) -> $t {
            match a.checked_sub(b) {
                Some(v) => v,
                None => trap(),
            }
        }

        #[doc = concat!("`-ftrapv`'s multiplication for `", stringify!($t), "`: aborts on overflow.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $mul(a: $t, b: $t) -> $t {
            match a.checked_mul(b) {
                Some(v) => v,
                None => trap(),
            }
        }
    )*};
}

trapping!(
    (__absvsi2, __negvsi2, __addvsi3, __subvsi3, __mulvsi3, i32),
    (__absvdi2, __negvdi2, __addvdi3, __subvdi3, __mulvdi3, i64),
    (__absvti2, __negvti2, __addvti3, __subvti3, __mulvti3, i128),
);

// -- plain integer helpers ---------------------------------------------------------

/// Two's-complement negation of a 64-bit integer (wraps, as C's does on this target).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __negdi2(a: i64) -> i64 {
    a.wrapping_neg()
}

/// Two's-complement negation of a 128-bit integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __negti2(a: i128) -> i128 {
    a.wrapping_neg()
}

/// libgcc's three-way comparison: 0 if `a < b`, 1 if equal, 2 if `a > b`.
fn three_way<T: Ord>(a: T, b: T) -> i32 {
    match a.cmp(&b) {
        core::cmp::Ordering::Less => 0,
        core::cmp::Ordering::Equal => 1,
        core::cmp::Ordering::Greater => 2,
    }
}

/// Signed 64-bit three-way comparison (0, 1 or 2).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cmpdi2(a: i64, b: i64) -> i32 {
    three_way(a, b)
}

/// Signed 128-bit three-way comparison (0, 1 or 2).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __cmpti2(a: i128, b: i128) -> i32 {
    three_way(a, b)
}

/// Unsigned 64-bit three-way comparison (0, 1 or 2).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ucmpdi2(a: u64, b: u64) -> i32 {
    three_way(a, b)
}

/// Unsigned 128-bit three-way comparison (0, 1 or 2).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ucmpti2(a: u128, b: u128) -> i32 {
    three_way(a, b)
}

macro_rules! bit_counts {
    ($(($parity:ident, $popcount:ident, $t:ty)),* $(,)?) => {$(
        #[doc = concat!("1 if an odd number of `", stringify!($t), "`'s bits are set, else 0.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $parity(a: $t) -> i32 {
            // The bits, not the value: reinterpreting the sign bit is the point.
            i32::from(a.cast_unsigned().count_ones() % 2 == 1)
        }

        #[doc = concat!("How many of `", stringify!($t), "`'s bits are set.")]
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $popcount(a: $t) -> i32 {
            // At most 128, which an i32 holds.
            i32::try_from(a.cast_unsigned().count_ones()).unwrap_or(i32::MAX)
        }
    )*};
}

bit_counts!(
    (__paritysi2, __popcountsi2, i32),
    (__paritydi2, __popcountdi2, i64),
    (__parityti2, __popcountti2, i128),
);

/// One more than the index of the least significant set bit of `a`, or 0 if
/// no bit is set: C's `ffs` for a 128-bit integer.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __ffsti2(a: i128) -> i32 {
    if a == 0 {
        0
    } else {
        // At most 128, so both conversions hold.
        i32::try_from(a.trailing_zeros()).map_or(i32::MAX, |z| z.saturating_add(1))
    }
}

// -- floating point ------------------------------------------------------------------

/// Negate a `float` by flipping its sign bit, NaN payloads included.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __negsf2(a: f32) -> f32 {
    f32::from_bits(a.to_bits() ^ 0x8000_0000)
}

/// Negate a `double` by flipping its sign bit, NaN payloads included.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __negdf2(a: f64) -> f64 {
    f64::from_bits(a.to_bits() ^ 0x8000_0000_0000_0000)
}

/// 1 with `x`'s sign if `x` is infinite, else 0 with it: Annex G's "box" of an
/// infinite operand.
fn unit_if_inf64(x: f64) -> f64 {
    (if x.is_infinite() { 1.0 } else { 0.0_f64 }).copysign(x)
}

fn zero_if_nan64(x: f64) -> f64 {
    if x.is_nan() { 0.0_f64.copysign(x) } else { x }
}

/// `(a + bi) * (c + di)` for `double _Complex`, with C99 Annex G's recovery of
/// infinities that the naive formula turns into NaN.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __muldc3(a: f64, b: f64, c: f64, d: f64) -> Complex64 {
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let ac = a * c;
    let bd = b * d;
    let ad = a * d;
    let bc = b * c;
    let mut z = Complex64 {
        re: ac - bd,
        im: ad + bc,
    };
    if z.re.is_nan() && z.im.is_nan() {
        let mut recalc = false;
        if a.is_infinite() || b.is_infinite() {
            a = unit_if_inf64(a);
            b = unit_if_inf64(b);
            c = zero_if_nan64(c);
            d = zero_if_nan64(d);
            recalc = true;
        }
        if c.is_infinite() || d.is_infinite() {
            c = unit_if_inf64(c);
            d = unit_if_inf64(d);
            a = zero_if_nan64(a);
            b = zero_if_nan64(b);
            recalc = true;
        }
        if !recalc && (ac.is_infinite() || bd.is_infinite() || ad.is_infinite() || bc.is_infinite())
        {
            a = zero_if_nan64(a);
            b = zero_if_nan64(b);
            c = zero_if_nan64(c);
            d = zero_if_nan64(d);
            recalc = true;
        }
        if recalc {
            z.re = f64::INFINITY * (a * c - b * d);
            z.im = f64::INFINITY * (a * d + b * c);
        }
    }
    z
}

/// `(a + bi) / (c + di)` for `double _Complex`: scaled by the divisor's
/// exponent against overflow, with Annex G's recovery of infinities and zeros.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __divdc3(a: f64, b: f64, c: f64, d: f64) -> Complex64 {
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let mut ilogbw = 0_i32;
    let logbw = crate::math::logb(c.abs().max(d.abs()));
    if logbw.is_finite() {
        // A finite logb of a double is within +/-1074, which an i32 holds.
        #[allow(clippy::cast_possible_truncation)]
        let e = logbw as i32;
        ilogbw = e;
        c = crate::math::scalbn(c, ilogbw.saturating_neg());
        d = crate::math::scalbn(d, ilogbw.saturating_neg());
    }
    let denom = c * c + d * d;
    let mut z = Complex64 {
        re: crate::math::scalbn((a * c + b * d) / denom, ilogbw.saturating_neg()),
        im: crate::math::scalbn((b * c - a * d) / denom, ilogbw.saturating_neg()),
    };
    if z.re.is_nan() && z.im.is_nan() {
        // Exactly zero, as compiler-rt compares: the divisor vanished.
        #[allow(clippy::float_cmp)]
        let zero_divisor = denom == 0.0;
        if zero_divisor && (!a.is_nan() || !b.is_nan()) {
            z.re = f64::INFINITY.copysign(c) * a;
            z.im = f64::INFINITY.copysign(c) * b;
        } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
            a = unit_if_inf64(a);
            b = unit_if_inf64(b);
            z.re = f64::INFINITY * (a * c + b * d);
            z.im = f64::INFINITY * (b * c - a * d);
        } else if logbw.is_infinite() && logbw > 0.0 && a.is_finite() && b.is_finite() {
            c = unit_if_inf64(c);
            d = unit_if_inf64(d);
            z.re = 0.0 * (a * c + b * d);
            z.im = 0.0 * (b * c - a * d);
        }
    }
    z
}

fn unit_if_inf32(x: f32) -> f32 {
    (if x.is_infinite() { 1.0 } else { 0.0_f32 }).copysign(x)
}

fn zero_if_nan32(x: f32) -> f32 {
    if x.is_nan() { 0.0_f32.copysign(x) } else { x }
}

/// `(a + bi) * (c + di)` for `float _Complex`; see [`__muldc3`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __mulsc3(a: f32, b: f32, c: f32, d: f32) -> Complex32 {
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let ac = a * c;
    let bd = b * d;
    let ad = a * d;
    let bc = b * c;
    let mut z = Complex32 {
        re: ac - bd,
        im: ad + bc,
    };
    if z.re.is_nan() && z.im.is_nan() {
        let mut recalc = false;
        if a.is_infinite() || b.is_infinite() {
            a = unit_if_inf32(a);
            b = unit_if_inf32(b);
            c = zero_if_nan32(c);
            d = zero_if_nan32(d);
            recalc = true;
        }
        if c.is_infinite() || d.is_infinite() {
            c = unit_if_inf32(c);
            d = unit_if_inf32(d);
            a = zero_if_nan32(a);
            b = zero_if_nan32(b);
            recalc = true;
        }
        if !recalc && (ac.is_infinite() || bd.is_infinite() || ad.is_infinite() || bc.is_infinite())
        {
            a = zero_if_nan32(a);
            b = zero_if_nan32(b);
            c = zero_if_nan32(c);
            d = zero_if_nan32(d);
            recalc = true;
        }
        if recalc {
            z.re = f32::INFINITY * (a * c - b * d);
            z.im = f32::INFINITY * (a * d + b * c);
        }
    }
    z
}

/// `(a + bi) / (c + di)` for `float _Complex`; see [`__divdc3`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn __divsc3(a: f32, b: f32, c: f32, d: f32) -> Complex32 {
    let (mut a, mut b, mut c, mut d) = (a, b, c, d);
    let mut ilogbw = 0_i32;
    let logbw = crate::math::logbf(c.abs().max(d.abs()));
    if logbw.is_finite() {
        // A finite logbf of a float is within +/-149.
        #[allow(clippy::cast_possible_truncation)]
        let e = logbw as i32;
        ilogbw = e;
        c = crate::math::scalbnf(c, ilogbw.saturating_neg());
        d = crate::math::scalbnf(d, ilogbw.saturating_neg());
    }
    let denom = c * c + d * d;
    let mut z = Complex32 {
        re: crate::math::scalbnf((a * c + b * d) / denom, ilogbw.saturating_neg()),
        im: crate::math::scalbnf((b * c - a * d) / denom, ilogbw.saturating_neg()),
    };
    if z.re.is_nan() && z.im.is_nan() {
        // Exactly zero, as compiler-rt compares: the divisor vanished.
        #[allow(clippy::float_cmp)]
        let zero_divisor = denom == 0.0;
        if zero_divisor && (!a.is_nan() || !b.is_nan()) {
            z.re = f32::INFINITY.copysign(c) * a;
            z.im = f32::INFINITY.copysign(c) * b;
        } else if (a.is_infinite() || b.is_infinite()) && c.is_finite() && d.is_finite() {
            a = unit_if_inf32(a);
            b = unit_if_inf32(b);
            z.re = f32::INFINITY * (a * c + b * d);
            z.im = f32::INFINITY * (b * c - a * d);
        } else if logbw.is_infinite() && logbw > 0.0 && a.is_finite() && b.is_finite() {
            c = unit_if_inf32(c);
            d = unit_if_inf32(d);
            z.re = 0.0 * (a * c + b * d);
            z.im = 0.0 * (b * c - a * d);
        }
    }
    z
}

#[cfg(test)]
mod tests {
    use super::*;

    const INF: f64 = f64::INFINITY;
    const NAN: f64 = f64::NAN;

    #[test]
    fn trapping_arithmetic_that_does_not_overflow_is_plain_arithmetic() {
        assert_eq!(__absvsi2(-5), 5);
        assert_eq!(__absvdi2(i64::MIN + 1), i64::MAX);
        assert_eq!(__absvti2(-7), 7);
        assert_eq!(__negvdi2(3), -3);
        assert_eq!(__addvsi3(i32::MAX - 1, 1), i32::MAX);
        assert_eq!(__subvdi3(i64::MIN + 1, 1), i64::MIN);
        assert_eq!(__mulvti3(1 << 62, 4), 1_i128 << 64);
        assert_eq!(__mulvsi3(-46_341, 46_340), -2_147_441_940);
    }

    #[test]
    fn plain_integer_helpers() {
        assert_eq!(__negdi2(i64::MIN), i64::MIN, "wraps");
        assert_eq!(__negti2(5), -5);
        assert_eq!(
            (__cmpdi2(-1, 0), __cmpdi2(3, 3), __cmpdi2(4, -9)),
            (0, 1, 2)
        );
        assert_eq!((__cmpti2(i128::MIN, 0), __cmpti2(0, 0)), (0, 1));
        assert_eq!(__ucmpdi2(u64::MAX, 0), 2, "unsigned: MAX is the largest");
        assert_eq!(__ucmpti2(1, u128::MAX), 0);
    }

    #[test]
    fn bit_counts_see_bits_not_values() {
        assert_eq!(__popcountsi2(-1), 32);
        assert_eq!(__popcountdi2(0b1011), 3);
        assert_eq!(__popcountti2(-1), 128);
        assert_eq!(__paritysi2(0b111), 1);
        assert_eq!(__paritydi2(0b11), 0);
        assert_eq!(__parityti2(i128::MIN), 1, "the sign bit counts");
        assert_eq!(__ffsti2(0), 0);
        assert_eq!(__ffsti2(1), 1);
        assert_eq!(__ffsti2(1 << 100), 101);
        assert_eq!(__ffsti2(i128::MIN), 128);
    }

    #[test]
    fn negation_flips_only_the_sign_bit() {
        assert_eq!(__negdf2(1.5), -1.5);
        assert_eq!(__negsf2(-0.0).to_bits(), 0.0_f32.to_bits());
        let nan = f64::from_bits(0x7ff8_0000_0000_1234);
        assert_eq!(
            __negdf2(nan).to_bits(),
            0xfff8_0000_0000_1234,
            "payload kept"
        );
    }

    #[test]
    fn complex_multiplication_of_finite_values_is_the_textbook_formula() {
        // (1+2i)(3+4i) = -5 + 10i
        assert_eq!(
            __muldc3(1.0, 2.0, 3.0, 4.0),
            Complex64 { re: -5.0, im: 10.0 }
        );
        assert_eq!(
            __mulsc3(1.0, 2.0, 3.0, 4.0),
            Complex32 { re: -5.0, im: 10.0 }
        );
    }

    /// Annex G: an infinite operand times a nonzero finite one is infinite,
    /// where the naive formula gives NaN in both parts.
    #[test]
    fn complex_multiplication_recovers_infinities() {
        let z = __muldc3(INF, NAN, 1.0, 1.0);
        assert!(z.re.is_infinite() || z.im.is_infinite(), "{z:?}");
        let z = __muldc3(1.0, 1.0, INF, INF);
        assert!(z.re.is_nan() || z.re.is_infinite());
        assert!(z.im.is_infinite(), "{z:?}");
        let z = __mulsc3(f32::INFINITY, 0.0, 2.0, 0.0);
        assert!(z.re.is_infinite(), "{z:?}");
    }

    #[test]
    fn complex_division_of_finite_values() {
        // (1+2i)/(3+4i) = (11 + 2i)/25
        let z = __divdc3(1.0, 2.0, 3.0, 4.0);
        assert!(
            (z.re - 0.44).abs() < 1e-15 && (z.im - 0.08).abs() < 1e-15,
            "{z:?}"
        );
        let z = __divsc3(1.0, 2.0, 3.0, 4.0);
        assert!(
            (z.re - 0.44).abs() < 1e-6 && (z.im - 0.08).abs() < 1e-6,
            "{z:?}"
        );
        // Scaling keeps a huge divisor from overflowing its own square.
        let z = __divdc3(1e300, 1e300, 1e300, 1e300);
        assert!((z.re - 1.0).abs() < 1e-15 && z.im.abs() < 1e-15, "{z:?}");
    }

    #[test]
    fn complex_division_recovers_infinities_and_zeros() {
        // Nonzero / zero is infinite, with the numerator's signs.
        let z = __divdc3(1.0, -1.0, 0.0, 0.0);
        assert_eq!((z.re, z.im), (INF, -INF));
        // Infinite / finite is infinite.
        let z = __divdc3(INF, 0.0, 2.0, 0.0);
        assert!(z.re.is_infinite(), "{z:?}");
        // Finite / infinite is zero.
        let z = __divdc3(1.0, 1.0, INF, 0.0);
        assert_eq!((z.re, z.im), (0.0, 0.0));
        let z = __divsc3(1.0, 1.0, 0.0, 0.0);
        assert_eq!(z.re, f32::INFINITY);
    }
}

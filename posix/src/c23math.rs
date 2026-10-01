//! C23's exact `<math.h>` functions, and glibc's: the ones that never round
//! -- IEEE 754-2019's nextUp and nextDown (`nextup`, `nextdown`), logB into a
//! `long` (`llogb`), canonicalize, convertToInteger with an explicit direction
//! and width (`fromfp`, `ufromfp`, `fromfpx`, `ufromfpx`), the NaN payload
//! operations (`getpayload`, `setpayload`, `setpayloadsig`), totalOrder and
//! totalOrderMag, the ten maximum and minimum operations (`fmaxmag`,
//! `fminmag`, `fmaximum`, `fminimum`, `fmaximum_num`, `fminimum_num` and their
//! `_mag` forms) -- for `double`, `float` and `long double`, and XSI's
//! obsolete `scalbl`.
//!
//! Each is glibc 2.39's code, turned into Rust where it is C: dbl-64's and
//! flt-32's bit manipulation over one [`Binary`] trait for the two IEEE
//! formats, ldbl-96's for the x87's 80 bits, and math/'s templates for the
//! maximum and minimum functions. They are exact, so glibc's answers are the
//! only right ones -- down to which NaN comes back, which flags are raised
//! and what `errno` says -- and `c23math_oracle.txt`
//! (`posix/tools/oracle/c23math_harness.py`) holds every function to them,
//! including the 80-bit encodings the x87 refuses (unnormals, pseudo-infinity,
//! pseudo-NaN), which glibc answers by whatever its code does with them. Where
//! glibc's C performs a floating-point operation -- `x + x` to quiet a
//! signaling NaN, `isinf` on an x87 value -- this performs the same one, so
//! the processor raises what it raised there.
//!
//! The C ABI: `double` and `float` are ordinary `extern "C"` functions;
//! `long double` ones go through [`crate::ld_c`]'s thunks, as `mathl.rs`'s do;
//! the functions of pointers (`canonicalize`, `totalorder`, ...) are `unsafe`,
//! reading and writing where the caller points, as C's do.

#![allow(clippy::arithmetic_side_effects)]
// exponent and bit arithmetic on encodings the formats bound
// The bit patterns are the formats' own fields, so the casts between u64,
// i32 and the fields' widths are where the code is exact by construction.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use crate::errno;
use crate::fenv::{FE_INEXACT, FE_INVALID};
use crate::x87::LongDouble as L;

/// glibc's `FP_INT_UPWARD` ... `FP_INT_TONEAREST`: `fromfp`'s directions.
const FP_INT_UPWARD: i32 = 0;
const FP_INT_DOWNWARD: i32 = 1;
const FP_INT_TONEARESTFROMZERO: i32 = 3;
const FP_INT_TONEAREST: i32 = 4;

/// Raise `excepts`, as glibc's `feraiseexcept` does -- by an operation that
/// raises it, so the flag is the unit's own.
fn raise(excepts: i32) {
    // The return value only reports an argument outside FE_ALL_EXCEPT.
    let _ = crate::fenv::feraiseexcept(excepts);
}

// ===========================================================================
// binary32 and binary64
// ===========================================================================

/// The two IEEE binary formats, as glibc's flt-32 and dbl-64 code sees them:
/// their bits (in a `u64`), and the operations that must be the format's own.
pub(crate) trait Binary: Copy {
    /// Stored significand bits: 23, 52.
    const MANT: u32;
    /// Exponent bits: 8, 11.
    const EXP: u32;
    /// The bits, zero-extended.
    fn bits(self) -> u64;
    /// The value of the low bits.
    fn from_bits(b: u64) -> Self;
    /// `self + other`, `self` the first operand: SSE's addition answers the
    /// first operand's NaN when both are NaNs, as glibc's compiled `x + y`
    /// does -- so the order is fixed here, where the compiler may swap a
    /// commutative `+`.
    fn add(self, other: Self) -> Self;
    /// `isgreater`, quiet: invalid only for a signaling NaN.
    fn gt(self, other: Self) -> bool;
    /// `isless`, quiet.
    fn lt(self, other: Self) -> bool;
    /// `==`, quiet.
    fn eq(self, other: Self) -> bool;
    /// An integer below 2^(MANT - 1), exactly (glibc's `(double) ix`).
    fn from_small(n: u64) -> Self;
    /// `-1` (`getpayload`'s answer for a number).
    const MINUS_ONE: Self;

    /// The sign bit.
    const SIGN: u64 = 1 << (Self::EXP + Self::MANT);
    /// The exponent field, all ones.
    const EXPMASK: u64 = ((1 << Self::EXP) - 1) << Self::MANT;
    /// The significand field.
    const MANTMASK: u64 = (1 << Self::MANT) - 1;
    /// The quiet bit: the significand's top.
    const QUIET: u64 = 1 << (Self::MANT - 1);
    /// The exponent bias.
    const BIAS: i32 = (1 << (Self::EXP - 1)) - 1;
}

impl Binary for f64 {
    const MANT: u32 = 52;
    const EXP: u32 = 11;
    const MINUS_ONE: Self = -1.0;
    fn bits(self) -> u64 {
        self.to_bits()
    }
    fn from_bits(b: u64) -> Self {
        f64::from_bits(b)
    }
    fn add(self, other: Self) -> Self {
        #[cfg(target_feature = "sse")]
        {
            let mut x = self;
            // SAFETY: one `addsd` between two registers; it touches no
            // memory and changes nothing but its destination and MXCSR's
            // flags.
            unsafe {
                core::arch::asm!("addsd {x}, {y}", x = inout(xmm_reg) x, y = in(xmm_reg) other,
                    options(nomem, nostack));
            }
            x
        }
        // The soft-float `cargo check` build (fenv.rs's `raise_by_division`
        // says why it exists) has no SSE unit, and so no NaN order to keep.
        #[cfg(not(target_feature = "sse"))]
        {
            self + other
        }
    }
    fn gt(self, other: Self) -> bool {
        self > other
    }
    fn lt(self, other: Self) -> bool {
        self < other
    }
    fn eq(self, other: Self) -> bool {
        self == other
    }
    fn from_small(n: u64) -> Self {
        n as f64
    }
}

impl Binary for f32 {
    const MANT: u32 = 23;
    const EXP: u32 = 8;
    const MINUS_ONE: Self = -1.0;
    fn bits(self) -> u64 {
        u64::from(self.to_bits())
    }
    fn from_bits(b: u64) -> Self {
        f32::from_bits(b as u32)
    }
    fn add(self, other: Self) -> Self {
        #[cfg(target_feature = "sse")]
        {
            let mut x = self;
            // SAFETY: one `addss` between two registers, as `f64`'s `addsd`.
            unsafe {
                core::arch::asm!("addss {x}, {y}", x = inout(xmm_reg) x, y = in(xmm_reg) other,
                    options(nomem, nostack));
            }
            x
        }
        // As `f64`'s.
        #[cfg(not(target_feature = "sse"))]
        {
            self + other
        }
    }
    fn gt(self, other: Self) -> bool {
        self > other
    }
    fn lt(self, other: Self) -> bool {
        self < other
    }
    fn eq(self, other: Self) -> bool {
        self == other
    }
    fn from_small(n: u64) -> Self {
        n as f32
    }
}

fn is_nan<T: Binary>(x: T) -> bool {
    x.bits() & !T::SIGN > T::EXPMASK
}

fn signaling<T: Binary>(x: T) -> bool {
    is_nan(x) && x.bits() & T::QUIET == 0
}

fn negate<T: Binary>(x: T) -> T {
    T::from_bits(x.bits() ^ T::SIGN)
}

fn fabs<T: Binary>(x: T) -> T {
    T::from_bits(x.bits() & !T::SIGN)
}

fn negative<T: Binary>(x: T) -> bool {
    x.bits() & T::SIGN != 0
}

/// glibc's `s_nextup.c`: the least value above `x` -- a NaN quieted
/// (`x + x`), either zero the least subnormal, `+inf` itself, and otherwise
/// one step in the encoding, toward zero for a negative `x`.
fn nextup_bits<T: Binary>(x: T) -> T {
    let b = x.bits();
    let magnitude = b & !T::SIGN;
    if magnitude > T::EXPMASK {
        return x.add(x);
    }
    if magnitude == 0 {
        return T::from_bits(1);
    }
    if b & T::SIGN == 0 {
        if magnitude == T::EXPMASK {
            return x;
        }
        T::from_bits(b + 1)
    } else {
        T::from_bits(b - 1)
    }
}

/// glibc's `s_nextdown_template.c`: `-nextup(-x)`.
fn nextdown_bits<T: Binary>(x: T) -> T {
    negate(nextup_bits(negate(x)))
}

/// glibc's `s_canonicalize_template.c`: every binary32 and binary64 encoding
/// is canonical, so only a signaling NaN changes -- quieted, raising invalid.
fn canonical<T: Binary>(x: T) -> T {
    if signaling(x) { x.add(x) } else { x }
}

/// glibc's `s_getpayload.c`: a NaN's payload -- the significand below the
/// quiet bit -- as a number; `-1` for anything else.
fn payload_of<T: Binary>(x: T) -> T {
    let b = x.bits();
    if b & T::EXPMASK != T::EXPMASK || b & T::MANTMASK == 0 {
        return T::MINUS_ONE;
    }
    T::from_small(b & (T::QUIET - 1))
}

/// glibc's `s_setpayload_main.c`: the NaN whose payload is `payload`, quiet
/// (`setpayload`) or signaling (`setpayloadsig`); `None` -- and `+0` stored --
/// for a payload that is negative, not an integer, or too wide for the
/// significand below the quiet bit (and zero, which would make a signaling
/// NaN an infinity).
fn with_payload<T: Binary>(payload: T, signaling: bool) -> Option<T> {
    let payload_dig = T::MANT as i32 - 1;
    let mut ix = payload.bits();
    // The sign bit is part of the "exponent" here, so a negative payload is
    // one too large.
    let exponent = (ix >> T::MANT) as i32;
    let set_high = !signaling;
    if exponent >= T::BIAS + payload_dig
        || (exponent < T::BIAS && !(set_high && ix == 0))
        || (exponent >= T::BIAS && ix & ((1 << (T::BIAS + T::MANT as i32 - exponent)) - 1) != 0)
    {
        return None;
    }
    if ix != 0 {
        ix &= T::MANTMASK;
        ix |= 1 << T::MANT;
        ix >>= T::BIAS + T::MANT as i32 - exponent;
    }
    ix |= T::EXPMASK | if set_high { T::QUIET } else { 0 };
    Some(T::from_bits(ix))
}

/// glibc's `s_totalorder.c`: the encodings as sign-magnitude integers, which
/// on x86 -- a quiet NaN's high significand bit set -- is IEEE's total order
/// as it stands.
fn total_order<T: Binary>(x: T, y: T) -> bool {
    let key = |v: T| {
        let b = v.bits();
        if b & T::SIGN != 0 {
            !b & (T::SIGN | (T::SIGN - 1))
        } else {
            b | T::SIGN
        }
    };
    key(x) <= key(y)
}

/// glibc's `s_totalordermag.c`: [`total_order`] of the magnitudes.
fn total_order_mag<T: Binary>(x: T, y: T) -> bool {
    x.bits() & !T::SIGN <= y.bits() & !T::SIGN
}

/// The ten maximum and minimum operations, glibc's `math/s_f*_template.c`
/// each: which comparison, and what a NaN does.
#[derive(Clone, Copy, Debug)]
pub enum MinMax {
    /// `fmaxmag`: the larger magnitude; a quiet NaN loses to a number.
    MaxMag,
    /// `fminmag`.
    MinMag,
    /// `fmaximum`: the larger, `+0` above `-0`; any NaN wins.
    Maximum,
    /// `fminimum`.
    Minimum,
    /// `fmaximum_num`: the larger; a NaN loses to a number.
    MaximumNum,
    /// `fminimum_num`.
    MinimumNum,
    /// `fmaximum_mag`: the larger magnitude, then as `fmaximum`.
    MaximumMag,
    /// `fminimum_mag`.
    MinimumMag,
    /// `fmaximum_mag_num`.
    MaximumMagNum,
    /// `fminimum_mag_num`.
    MinimumMagNum,
}

impl MinMax {
    fn larger(self) -> bool {
        matches!(
            self,
            Self::MaxMag
                | Self::Maximum
                | Self::MaximumNum
                | Self::MaximumMag
                | Self::MaximumMagNum
        )
    }

    fn by_magnitude(self) -> bool {
        matches!(
            self,
            Self::MaxMag
                | Self::MinMag
                | Self::MaximumMag
                | Self::MinimumMag
                | Self::MaximumMagNum
                | Self::MinimumMagNum
        )
    }
}

/// One of [`MinMax`]'s operations on `x` and `y`, as its template has it.
fn min_max<T: Binary>(op: MinMax, x: T, y: T) -> T {
    let (a, b) = if op.by_magnitude() {
        (fabs(x), fabs(y))
    } else {
        (x, y)
    };
    let (first, second) = if op.larger() {
        (a.gt(b), a.lt(b))
    } else {
        (a.lt(b), a.gt(b))
    };
    if first {
        return x;
    }
    if second {
        return y;
    }
    if a.eq(b) {
        return match op {
            // `x > y ? x : y` and `x < y ? x : y`: equal magnitudes, so the
            // numbers differ at most in sign.
            MinMax::MaxMag => {
                if x.gt(y) {
                    x
                } else {
                    y
                }
            }
            MinMax::MinMag => {
                if x.lt(y) {
                    x
                } else {
                    y
                }
            }
            // `copysign(1, x) >= copysign(1, y) ? x : y` -- the positive
            // zero above the negative -- and `<=` for the minimum.
            _ if op.larger() => {
                if !negative(x) || negative(y) {
                    x
                } else {
                    y
                }
            }
            _ => {
                if negative(x) || !negative(y) {
                    x
                } else {
                    y
                }
            }
        };
    }
    // At least one NaN.
    match op {
        MinMax::MaxMag | MinMax::MinMag => {
            if signaling(x) || signaling(y) {
                x.add(y)
            } else if is_nan(y) {
                x
            } else {
                y
            }
        }
        MinMax::Maximum | MinMax::Minimum | MinMax::MaximumMag | MinMax::MinimumMag => x.add(y),
        MinMax::MaximumNum | MinMax::MinimumNum | MinMax::MaximumMagNum | MinMax::MinimumMagNum => {
            if is_nan(y) {
                if is_nan(x) { x.add(y) } else { x }
            } else {
                y
            }
        }
    }
}

// ---------------------------------------------------------------------------
// fromfp: glibc's math/fromfp.h
// ---------------------------------------------------------------------------

/// Which of the four `fromfp` functions.
#[derive(Clone, Copy)]
struct FromFp {
    /// `ufromfp`, `ufromfpx`: the result is a `uintmax_t`.
    unsigned: bool,
    /// `fromfpx`, `ufromfpx`: an inexact result raises inexact.
    inexact: bool,
}

impl FromFp {
    /// `fromfp_max_exponent`: the largest unbiased exponent a value of this
    /// sign can have and still fit `width` bits.
    fn max_exponent(self, negative: bool, width: u32) -> i32 {
        let width = width as i32;
        match (self.unsigned, negative) {
            (true, true) => -1,
            (true, false) => width - 1,
            (false, true) => width - 1,
            (false, false) => width - 2,
        }
    }

    /// `fromfp_round`: the integer part `x` (a magnitude), rounded in
    /// `round` given the half bit and whether any bit below it is set. An
    /// unknown direction truncates.
    fn round(negative: bool, x: u64, half: bool, more: bool, round: i32) -> u64 {
        let up = match round {
            FP_INT_UPWARD => !negative && (half || more),
            FP_INT_DOWNWARD => negative && (half || more),
            FP_INT_TONEARESTFROMZERO => half,
            FP_INT_TONEAREST => half && (x & 1 != 0 || more),
            _ => false,
        };
        x.wrapping_add(u64::from(up))
    }

    /// `fromfp_overflowed`: whether the rounded `x` (possibly wrapped to 0)
    /// is past the width.
    fn overflowed(self, negative: bool, x: u64, exponent: i32, max_exponent: i32) -> bool {
        if self.unsigned {
            if negative {
                x != 0
            } else if max_exponent == 63 {
                exponent == 63 && x == 0
            } else {
                x == 1 << (max_exponent + 1)
            }
        } else if negative {
            exponent == max_exponent && x != 1 << max_exponent
        } else {
            x == 1 << (max_exponent + 1)
        }
    }

    /// `fromfp_domain_error`: invalid, `EDOM`, and the width's saturation.
    fn domain_error(self, negative: bool, width: u32) -> u64 {
        raise(FE_INVALID);
        errno::set_errno(errno::EDOM);
        if self.unsigned {
            if negative {
                0
            } else if width == 64 {
                u64::MAX
            } else {
                (1 << width) - 1
            }
        } else if width == 0 {
            0
        } else if negative {
            (1u64 << (width - 1)).wrapping_neg()
        } else {
            (1 << (width - 1)) - 1
        }
    }

    /// `fromfp_round_and_return`, the answer as the bits of an `intmax_t`
    /// or a `uintmax_t`.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        self,
        negative: bool,
        x: u64,
        half: bool,
        more: bool,
        round: i32,
        exponent: i32,
        max_exponent: i32,
        width: u32,
    ) -> u64 {
        let r = Self::round(negative, x, half, more, round);
        if self.overflowed(negative, r, exponent, max_exponent) {
            return self.domain_error(negative, width);
        }
        if self.inexact && (half || more) {
            raise(FE_INEXACT);
        }
        if !self.unsigned && negative {
            r.wrapping_neg()
        } else {
            r
        }
    }

    /// dbl-64's and flt-32's `s_fromfp_main.c`.
    fn binary<T: Binary>(self, x: T, round: i32, width: u32) -> u64 {
        let width = width.min(64);
        let bits = x.bits();
        let negative = bits & T::SIGN != 0;
        if width == 0 {
            return self.domain_error(negative, width);
        }
        let ix = bits & !T::SIGN;
        if ix == 0 {
            return 0;
        }
        let exponent = (ix >> T::MANT) as i32 - T::BIAS;
        let max_exponent = self.max_exponent(negative, width);
        if exponent > max_exponent {
            return self.domain_error(negative, width);
        }
        let mant_dig = T::MANT as i32 + 1;
        let ix = (ix & T::MANTMASK) | (1 << T::MANT);
        let (r, half, more) = if exponent >= mant_dig - 1 {
            (ix << (exponent - (mant_dig - 1)), false, false)
        } else if exponent >= -1 {
            let h = 1u64 << (mant_dig - 2 - exponent);
            (
                ix >> (mant_dig - 1 - exponent),
                ix & h != 0,
                ix & (h - 1) != 0,
            )
        } else {
            (0, false, true)
        };
        self.finish(
            negative,
            r,
            half,
            more,
            round,
            exponent,
            max_exponent,
            width,
        )
    }

    /// ldbl-96's `s_fromfpl_main.c`: the explicit integer bit is the
    /// significand's own, and an all-zero significand is zero whatever its
    /// exponent says.
    fn extended(self, x: L, round: i32, width: u32) -> u64 {
        let width = width.min(64);
        let negative = x.sign_exp & 0x8000 != 0;
        if width == 0 {
            return self.domain_error(negative, width);
        }
        let ix = x.significand;
        if ix == 0 {
            return 0;
        }
        let exponent = i32::from(x.sign_exp & 0x7FFF) - 0x3FFF;
        let max_exponent = self.max_exponent(negative, width);
        if exponent > max_exponent {
            return self.domain_error(negative, width);
        }
        let (r, half, more) = if exponent >= 63 {
            (ix, false, false)
        } else if exponent >= -1 {
            let h = 1u64 << (62 - exponent);
            let r = if exponent == -1 {
                0
            } else {
                ix >> (63 - exponent)
            };
            (r, ix & h != 0, ix & (h - 1) != 0)
        } else {
            (0, false, true)
        };
        self.finish(
            negative,
            r,
            half,
            more,
            round,
            exponent,
            max_exponent,
            width,
        )
    }
}

const FROMFP: FromFp = FromFp {
    unsigned: false,
    inexact: false,
};
const FROMFPX: FromFp = FromFp {
    unsigned: false,
    inexact: true,
};
const UFROMFP: FromFp = FromFp {
    unsigned: true,
    inexact: false,
};
const UFROMFPX: FromFp = FromFp {
    unsigned: true,
    inexact: true,
};

// ---------------------------------------------------------------------------
// The C entry points: double and float
// ---------------------------------------------------------------------------

macro_rules! binary_exports {
    ($t:ty, $nextup:ident, $nextdown:ident, $llogb:ident, $ilogb:path, $canonicalize:ident,
     $getpayload:ident, $setpayload:ident, $setpayloadsig:ident, $totalorder:ident,
     $totalordermag:ident, $fromfp:ident, $fromfpx:ident, $ufromfp:ident, $ufromfpx:ident,
     [$($mm:ident $op:ident),* $(,)?]) => {
        /// The least value above `x` (IEEE 754's nextUp).
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $nextup(x: $t) -> $t {
            nextup_bits(x)
        }

        /// The greatest value below `x` (nextDown).
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $nextdown(x: $t) -> $t {
            nextdown_bits(x)
        }

        /// `ilogb` as a `long`, glibc's `w_llogb_template.c`: `LONG_MIN`
        /// for zero and NaN, `LONG_MAX` for an infinity, each with invalid
        /// and `EDOM`.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $llogb(x: $t) -> i64 {
            llogb_of($ilogb(x))
        }

        /// Store `*x` canonicalised in `*cx`: `0`, a signaling NaN quieted.
        ///
        /// # Safety
        ///
        /// `x` must be valid to read and `cx` to write, as C requires.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $canonicalize(cx: *mut $t, x: *const $t) -> i32 {
            // SAFETY: the caller's pointers, per the contract above.
            unsafe { cx.write(canonical(x.read())) };
            0
        }

        /// The payload of the NaN `*x`, as a number; `-1` if `*x` is not a NaN.
        ///
        /// # Safety
        ///
        /// `x` must be valid to read.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $getpayload(x: *const $t) -> $t {
            // SAFETY: the caller's pointer.
            payload_of(unsafe { x.read() })
        }

        /// Store the quiet NaN with payload `payload` in `*x`: `0`; or `+0`
        /// and `1` for a payload no NaN can carry.
        ///
        /// # Safety
        ///
        /// `x` must be valid to write.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $setpayload(x: *mut $t, payload: $t) -> i32 {
            let (v, r) = match with_payload(payload, false) {
                Some(v) => (v, 0),
                None => (<$t as Binary>::from_bits(0), 1),
            };
            // SAFETY: the caller's pointer.
            unsafe { x.write(v) };
            r
        }

        /// [`$setpayload`], signaling: a zero payload is refused too.
        ///
        /// # Safety
        ///
        /// `x` must be valid to write.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $setpayloadsig(x: *mut $t, payload: $t) -> i32 {
            let (v, r) = match with_payload(payload, true) {
                Some(v) => (v, 0),
                None => (<$t as Binary>::from_bits(0), 1),
            };
            // SAFETY: the caller's pointer.
            unsafe { x.write(v) };
            r
        }

        /// Whether `*x` is at or below `*y` in IEEE 754's total order.
        ///
        /// # Safety
        ///
        /// Both pointers must be valid to read.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $totalorder(x: *const $t, y: *const $t) -> i32 {
            // SAFETY: the caller's pointers.
            i32::from(total_order(unsafe { x.read() }, unsafe { y.read() }))
        }

        /// [`$totalorder`] of the magnitudes.
        ///
        /// # Safety
        ///
        /// Both pointers must be valid to read.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub unsafe extern "C" fn $totalordermag(x: *const $t, y: *const $t) -> i32 {
            // SAFETY: the caller's pointers.
            i32::from(total_order_mag(unsafe { x.read() }, unsafe { y.read() }))
        }

        /// `x` rounded to an integer in `round` (`FP_INT_*`), if it fits
        /// `width` bits signed; else invalid, `EDOM` and the width's
        /// saturation.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $fromfp(x: $t, round: i32, width: u32) -> i64 {
            FROMFP.binary(x, round, width) as i64
        }

        /// [`$fromfp`], raising inexact when the result is not `x`.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $fromfpx(x: $t, round: i32, width: u32) -> i64 {
            FROMFPX.binary(x, round, width) as i64
        }

        /// [`$fromfp`], unsigned.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $ufromfp(x: $t, round: i32, width: u32) -> u64 {
            UFROMFP.binary(x, round, width)
        }

        /// [`$ufromfp`], raising inexact when the result is not `x`.
        #[cfg_attr(target_os = "none", unsafe(no_mangle))]
        pub extern "C" fn $ufromfpx(x: $t, round: i32, width: u32) -> u64 {
            UFROMFPX.binary(x, round, width)
        }

        $(
            #[doc = concat!("`", stringify!($mm), "`: [`MinMax::", stringify!($op), "`].")]
            #[cfg_attr(target_os = "none", unsafe(no_mangle))]
            pub extern "C" fn $mm(x: $t, y: $t) -> $t {
                min_max(MinMax::$op, x, y)
            }
        )*
    };
}

/// glibc's `w_llogb_template.c` over an `ilogb` that has already raised
/// invalid and set `EDOM` for its three special answers: those widened to a
/// `long`'s.
fn llogb_of(r: i32) -> i64 {
    match r {
        i32::MIN => i64::MIN,
        i32::MAX => i64::MAX,
        r => i64::from(r),
    }
}

binary_exports!(
    f64, nextup, nextdown, llogb, crate::math::ilogb, canonicalize, getpayload, setpayload,
    setpayloadsig, totalorder, totalordermag, fromfp, fromfpx, ufromfp, ufromfpx,
    [
        fmaxmag MaxMag, fminmag MinMag, fmaximum Maximum, fminimum Minimum,
        fmaximum_num MaximumNum, fminimum_num MinimumNum, fmaximum_mag MaximumMag,
        fminimum_mag MinimumMag, fmaximum_mag_num MaximumMagNum, fminimum_mag_num MinimumMagNum,
    ]
);

binary_exports!(
    f32, nextupf, nextdownf, llogbf, crate::math::ilogbf, canonicalizef, getpayloadf, setpayloadf,
    setpayloadsigf, totalorderf, totalordermagf, fromfpf, fromfpxf, ufromfpf, ufromfpxf,
    [
        fmaxmagf MaxMag, fminmagf MinMag, fmaximumf Maximum, fminimumf Minimum,
        fmaximum_numf MaximumNum, fminimum_numf MinimumNum, fmaximum_magf MaximumMag,
        fminimum_magf MinimumMag, fmaximum_mag_numf MaximumMagNum,
        fminimum_mag_numf MinimumMagNum,
    ]
);

// ===========================================================================
// long double: ldbl-96
// ===========================================================================

/// glibc's ldbl-96 `issignalingl` with x86's `is_pseudo_signaling`: every
/// encoding the x87 refuses for its integer bit (a nonzero exponent with the
/// bit clear -- unnormals, pseudo-infinities, pseudo-NaNs) signals, as the
/// unit makes it; otherwise a NaN with its quiet bit clear.
fn signaling_l(x: L) -> bool {
    let exi = x.sign_exp & 0x7FFF;
    let hxi = (x.significand >> 32) as u32;
    let lxi = x.significand as u32;
    let pseudo = exi != 0 && hxi & 0x8000_0000 == 0;
    // The quiet bit toggled, so a signaling NaN's is set, and any low bit
    // folded into the high word: then "above the infinity's pattern".
    let h = (hxi ^ 0x4000_0000) | ((lxi | lxi.wrapping_neg()) >> 31);
    pseudo || (exi == 0x7FFF && h > 0xC000_0000)
}

/// ldbl-96's `s_iscanonicall.c`, the x87's rule: the integer bit set exactly
/// when the exponent is not zero. An unnormal, a pseudo-denormal, a
/// pseudo-infinity or a pseudo-NaN is not canonical.
fn canonical_l(x: L) -> bool {
    let high = x.significand >> 63 != 0;
    if x.sign_exp & 0x7FFF > 0 { high } else { !high }
}

/// glibc's `isinf` on an x87 value, which is a comparison of `|x|` with the
/// largest finite value on the x87 -- invalid for an operand the unit
/// refuses, as glibc's own raises it.
fn isinf_l(x: L) -> bool {
    x.abs().compare(L::from_bits(0x7FFE, u64::MAX)) == Some(core::cmp::Ordering::Greater)
}

/// ldbl-96's `s_nextupl.c`: a NaN (in its sense) quieted by `x + x`, a zero
/// the least subnormal, `+inf` itself; otherwise one step in the encoding,
/// with the integer bit kept right across the subnormal-normal boundary.
#[must_use]
pub fn nextupl(x: L) -> L {
    let mut esx = x.sign_exp;
    let mut hx = (x.significand >> 32) as u32;
    let mut lx = x.significand as u32;
    let ix = esx & 0x7FFF;
    if ix == 0x7FFF && ((hx & 0x7FFF_FFFF) | lx) != 0 {
        return x + x;
    }
    if (u32::from(ix) | hx | lx) == 0 {
        return L::from_bits(0, 1);
    }
    if esx & 0x8000 == 0 {
        if isinf_l(x) {
            return x;
        }
        lx = lx.wrapping_add(1);
        if lx == 0 {
            hx = hx.wrapping_add(1);
            if hx == 0 || (esx == 0 && hx == 0x8000_0000) {
                esx += 1;
                hx |= 0x8000_0000;
            }
        }
    } else {
        if lx == 0 {
            // glibc's `hx <= 0x80000000 && esx != 0xffff8000`, `esx` there
            // the sign-extended 16 bits: every negative but -0's encoding.
            if hx <= 0x8000_0000 && esx != 0x8000 {
                esx -= 1;
                hx = hx.wrapping_sub(1);
                if esx & 0x7FFF > 0 {
                    hx |= 0x8000_0000;
                }
            } else {
                hx = hx.wrapping_sub(1);
            }
        }
        lx = lx.wrapping_sub(1);
    }
    L::from_bits(esx, (u64::from(hx) << 32) | u64::from(lx))
}

/// `-nextupl(-x)`.
#[must_use]
pub fn nextdownl(x: L) -> L {
    nextupl(x.negate()).negate()
}

/// [`crate::mathl::ilogbl`] as a `long` (`llogbl`).
#[must_use]
pub fn llogbl(x: L) -> i64 {
    llogb_of(crate::mathl::ilogbl(x))
}

/// ldbl-96's `canonicalizel`'s answer: `None` for an encoding the x87 refuses, a
/// signaling NaN quieted by `x + x`, and anything else itself.
#[must_use]
pub fn canonical_value_l(x: L) -> Option<L> {
    if !canonical_l(x) {
        return None;
    }
    Some(if signaling_l(x) { x + x } else { x })
}

/// ldbl-96's `s_getpayloadl.c`: the payload -- the significand below the
/// quiet bit -- of anything with an all-ones exponent and a significand
/// other than the integer bit alone; `-1` for anything else.
#[must_use]
pub fn getpayloadl(x: L) -> L {
    let hx = (x.significand >> 32) as u32;
    let lx = x.significand as u32;
    if x.sign_exp & 0x7FFF != 0x7FFF || ((hx & 0x7FFF_FFFF) | lx) == 0 {
        return L::from_i64(-1);
    }
    let ix = (u64::from(hx & 0x3FFF_FFFF) << 32) | u64::from(lx);
    // Below 2^62, so an i64 holds it and the conversion is exact.
    L::from_i64(ix as i64)
}

/// ldbl-96's `s_setpayloadl_main.c`: the NaN with payload `payload`, quiet
/// or signaling, or `None` (and `+0` stored) for a payload no NaN can carry.
#[must_use]
pub fn with_payloadl(payload: L, signaling: bool) -> Option<L> {
    const BIAS: i32 = 0x3FFF;
    const PAYLOAD_DIG: i32 = 62;
    const EXPLICIT_MANT_DIG: i32 = 63;
    let set_high = !signaling;
    let exponent = i32::from(payload.sign_exp);
    let mut hx = (payload.significand >> 32) as u32;
    let mut lx = payload.significand as u32;
    if exponent >= BIAS + PAYLOAD_DIG
        || (exponent < BIAS && !(set_high && exponent == 0 && hx == 0 && lx == 0))
    {
        return None;
    }
    let shift = BIAS + EXPLICIT_MANT_DIG - exponent;
    // Only a zero payload can have an exponent below the bias here, and its
    // bits are all zero: nothing to shift.
    if exponent >= BIAS {
        let fractional = if shift < 32 {
            lx & ((1u32 << shift) - 1) != 0
        } else {
            lx != 0 || hx & ((1u32 << (shift - 32)) - 1) != 0
        };
        if fractional {
            return None;
        }
    }
    if exponent != 0 {
        if shift >= 32 {
            lx = hx >> (shift - 32);
            hx = 0;
        } else if shift != 0 {
            lx = (lx >> shift) | (hx << (32 - shift));
            hx >>= shift;
        }
    }
    hx |= 0x8000_0000 | if set_high { 0x4000_0000 } else { 0 };
    Some(L::from_bits(0x7FFF, (u64::from(hx) << 32) | u64::from(lx)))
}

/// ldbl-96's `s_totalorderl.c`: sign, then the exponent and significand
/// flipped for a negative value.
#[must_use]
pub fn total_order_l(x: L, y: L) -> bool {
    let key = |v: L| {
        let se = v.sign_exp as i16;
        if se < 0 {
            (se ^ 0x7FFF, !v.significand)
        } else {
            (se, v.significand)
        }
    };
    key(x) <= key(y)
}

/// ldbl-96's `s_totalordermagl.c`.
#[must_use]
pub fn total_order_mag_l(x: L, y: L) -> bool {
    (x.sign_exp & 0x7FFF, x.significand) <= (y.sign_exp & 0x7FFF, y.significand)
}

/// C's `isnan` on an x87 value as GCC compiles it, `x UNORD x`: a NaN, and
/// also an encoding the unit refuses -- unordered, and raising invalid.
fn isnan_l(v: L) -> bool {
    v.compare(v).is_none()
}

/// [`MinMax`]'s operations for `long double`, the same templates over the
/// x87's comparisons (quiet, as `isgreater` and `isless` are) and addition.
#[must_use]
pub fn min_max_l(op: MinMax, x: L, y: L) -> L {
    use core::cmp::Ordering::{Equal, Greater, Less};
    let (a, b) = if op.by_magnitude() {
        (x.abs(), y.abs())
    } else {
        (x, y)
    };
    let order = a.compare(b);
    let (first, second) = if op.larger() {
        (Greater, Less)
    } else {
        (Less, Greater)
    };
    if order == Some(first) {
        return x;
    }
    if order == Some(second) {
        return y;
    }
    if order == Some(Equal) {
        let xy = x.compare(y);
        return match op {
            MinMax::MaxMag => {
                if xy == Some(Greater) {
                    x
                } else {
                    y
                }
            }
            MinMax::MinMag => {
                if xy == Some(Less) {
                    x
                } else {
                    y
                }
            }
            _ if op.larger() => {
                if !x.is_sign_negative() || y.is_sign_negative() {
                    x
                } else {
                    y
                }
            }
            _ => {
                if x.is_sign_negative() || !y.is_sign_negative() {
                    x
                } else {
                    y
                }
            }
        };
    }
    match op {
        MinMax::MaxMag | MinMax::MinMag => {
            if signaling_l(x) || signaling_l(y) {
                x + y
            } else if isnan_l(y) {
                x
            } else {
                y
            }
        }
        MinMax::Maximum | MinMax::Minimum | MinMax::MaximumMag | MinMax::MinimumMag => x + y,
        MinMax::MaximumNum | MinMax::MinimumNum | MinMax::MaximumMagNum | MinMax::MinimumMagNum => {
            if isnan_l(y) {
                if isnan_l(x) { x + y } else { x }
            } else {
                y
            }
        }
    }
}

/// What the x87's `fxam` calls a NaN: an all-ones exponent, the integer bit
/// set, and a fraction. A pseudo-NaN (integer bit clear) is "unsupported".
fn fxam_nan(v: L) -> bool {
    v.sign_exp & 0x7FFF == 0x7FFF && v.significand >> 63 == 1 && v.significand << 1 != 0
}

/// What `fxam` calls an infinity: an all-ones exponent and the integer bit
/// alone.
fn fxam_inf(v: L) -> bool {
    v.sign_exp & 0x7FFF == 0x7FFF && v.significand == 1 << 63
}

/// The x87's default NaN from an invalid operation, raising invalid:
/// `fldz; fdiv %st`, as glibc makes it.
fn invalid_nan() -> L {
    L::POS_ZERO / core::hint::black_box(L::POS_ZERO)
}

/// glibc's x86-64 `e_scalbl.S`, operation for operation. `fn_` is `-inf`:
/// a NaN (invalid) for an infinite `x`, glibc's own quiet NaN for a NaN `x`
/// (no invalid even for a signaling one), and otherwise zero with `x`'s
/// sign. A NaN either side: `fn_ + x`. Otherwise `fn_` must be an integer
/// -- `frndint` in the current direction gives it back -- and the answer is
/// `fscale`'s; an `fn_` that is not one is a NaN (invalid). An encoding the
/// x87 refuses goes wherever those operations take it, raising invalid on
/// the way, as it does there: `frndint` of it is a NaN, which the unordered
/// comparison lets through to `fscale`.
fn scalbl_raw(x: L, fn_: L) -> L {
    if fxam_inf(fn_) && fn_.is_sign_negative() {
        if fxam_inf(x) {
            return invalid_nan();
        }
        if fxam_nan(x) {
            // `nan: .byte 0,0,0,0,0,0,0xff,0x7f`, a double loaded as is.
            return L::from_bits(0x7FFF, 0xF800_0000_0000_0000);
        }
        return L::POS_ZERO.copysign(x);
    }
    if fxam_nan(fn_) || fxam_nan(x) {
        return fn_ + x;
    }
    // `fcomip` jumps to the NaN only on "not equal"; an unordered result
    // sets ZF as equality does, and falls through.
    let r = fn_.round_int();
    if r.compare(fn_)
        .is_some_and(|o| o != core::cmp::Ordering::Equal)
    {
        return invalid_nan();
    }
    x.fscale(fn_)
}

/// XSI's `scalbl` (removed from POSIX in 2008; glibc keeps it): `x *
/// 2^fn_` for an integral `fn_` -- [`scalbl_raw`], under glibc's
/// `w_scalbl_compat.c`: `EDOM` for a NaN made from arguments that were not,
/// `ERANGE` for an infinity from finite ones or a zero from a nonzero `x`
/// and a finite `fn_`.
#[must_use]
pub fn scalbl(x: L, fn_: L) -> L {
    let z = scalbl_raw(x, fn_);
    // The wrapper's tests of `x` and `fn_` are the x87's comparisons, as GCC
    // compiles `isnan`, `isinf` and `!=`: each raises invalid for a signaling
    // NaN or an encoding the unit refuses, in C's order and no further than
    // `&&` evaluates. `z`, an x87 result, is always a plain encoding.
    if !z.is_finite() || z.is_zero() {
        if z.is_nan() {
            if !isnan_l(x) && !isnan_l(fn_) {
                errno::set_errno(errno::EDOM);
            }
        } else if z.is_infinite() {
            if !isinf_l(x) && !isinf_l(fn_) {
                errno::set_errno(errno::ERANGE);
            }
        } else if x.compare(L::POS_ZERO) != Some(core::cmp::Ordering::Equal) && !isinf_l(fn_) {
            errno::set_errno(errno::ERANGE);
        }
    }
    z
}

// ---------------------------------------------------------------------------
// The C entry points: long double
// ---------------------------------------------------------------------------

macro_rules! export_l_l {
    ($($c:literal $shim:ident $f:path;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, r: *mut L) {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument and a slot on the thunk's frame.
            unsafe { r.write($f(x.read())) };
        }
        crate::ld_c!(l_l $c => $shim);
    )* };
}

macro_rules! export_l_ll {
    ($($c:literal $shim:ident $f:expr;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, y: *const L, r: *mut L) {
            // SAFETY: called only by its thunk, with the caller's two stack
            // arguments and a slot on the thunk's frame.
            unsafe { r.write(($f)(x.read(), y.read())) };
        }
        crate::ld_c!(l_ll $c => $shim);
    )* };
}

/// `fromfpl`: [`FromFp`] over the x87's format, signed.
#[must_use]
pub fn fromfpl(x: L, round: i32, width: u32) -> i64 {
    FROMFP.extended(x, round, width) as i64
}

/// `fromfpxl`: [`fromfpl`], raising inexact when the result is not `x`.
#[must_use]
pub fn fromfpxl(x: L, round: i32, width: u32) -> i64 {
    FROMFPX.extended(x, round, width) as i64
}

/// `ufromfpl`: [`fromfpl`], unsigned.
#[must_use]
pub fn ufromfpl(x: L, round: i32, width: u32) -> u64 {
    UFROMFP.extended(x, round, width)
}

/// `ufromfpxl`: [`ufromfpl`], raising inexact when the result is not `x`.
#[must_use]
pub fn ufromfpxl(x: L, round: i32, width: u32) -> u64 {
    UFROMFPX.extended(x, round, width)
}

macro_rules! export_fromfpl {
    ($($c:literal $shim:ident $f:ident $ty:ty;)*) => { $(
        #[cfg(target_os = "none")]
        #[unsafe(no_mangle)]
        unsafe extern "C" fn $shim(x: *const L, round: i32, width: u32) -> $ty {
            // SAFETY: called only by its thunk, with the caller's stack
            // argument.
            $f(unsafe { x.read() }, round, width)
        }
        crate::ld_c!(n_lii $c => $shim);
    )* };
}

export_l_l! {
    "nextupl" __slate_ld_nextupl nextupl;
    "nextdownl" __slate_ld_nextdownl nextdownl;
}

export_l_ll! {
    "fmaxmagl" __slate_ld_fmaxmagl |x, y| min_max_l(MinMax::MaxMag, x, y);
    "fminmagl" __slate_ld_fminmagl |x, y| min_max_l(MinMax::MinMag, x, y);
    "fmaximuml" __slate_ld_fmaximuml |x, y| min_max_l(MinMax::Maximum, x, y);
    "fminimuml" __slate_ld_fminimuml |x, y| min_max_l(MinMax::Minimum, x, y);
    "fmaximum_numl" __slate_ld_fmaximum_numl |x, y| min_max_l(MinMax::MaximumNum, x, y);
    "fminimum_numl" __slate_ld_fminimum_numl |x, y| min_max_l(MinMax::MinimumNum, x, y);
    "fmaximum_magl" __slate_ld_fmaximum_magl |x, y| min_max_l(MinMax::MaximumMag, x, y);
    "fminimum_magl" __slate_ld_fminimum_magl |x, y| min_max_l(MinMax::MinimumMag, x, y);
    "fmaximum_mag_numl" __slate_ld_fmaximum_mag_numl |x, y| min_max_l(MinMax::MaximumMagNum, x, y);
    "fminimum_mag_numl" __slate_ld_fminimum_mag_numl |x, y| min_max_l(MinMax::MinimumMagNum, x, y);
    "scalbl" __slate_ld_scalbl scalbl;
}

export_fromfpl! {
    "fromfpl" __slate_ld_fromfpl fromfpl i64;
    "fromfpxl" __slate_ld_fromfpxl fromfpxl i64;
    "ufromfpl" __slate_ld_ufromfpl ufromfpl u64;
    "ufromfpxl" __slate_ld_ufromfpxl ufromfpxl u64;
}

/// `llogbl`'s thunk target.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_llogbl(x: *const L) -> i64 {
    // SAFETY: called only by its thunk, with the caller's stack argument.
    llogbl(unsafe { x.read() })
}
crate::ld_c!(i_l "llogbl" => __slate_ld_llogbl);

/// `getpayloadl(const long double *x)`'s thunk target: no long double
/// argument by value, only the result's slot.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_getpayloadl(x: *const L, r: *mut L) {
    // SAFETY: the caller's pointer, and the thunk's slot.
    unsafe { r.write(getpayloadl(x.read())) };
}
crate::ld_c!(l_p "getpayloadl" => __slate_ld_getpayloadl);

/// `setpayloadl(long double *x, long double payload)`'s thunk target.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_setpayloadl(x: *mut L, payload: *const L) -> i32 {
    // SAFETY: the thunk's arguments, as `store_payload_l` needs them.
    unsafe { store_payload_l(x, payload, false) }
}
crate::ld_c!(i_pl "setpayloadl" => __slate_ld_setpayloadl);

/// `setpayloadsigl`'s thunk target.
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
unsafe extern "C" fn __slate_ld_setpayloadsigl(x: *mut L, payload: *const L) -> i32 {
    // SAFETY: as `__slate_ld_setpayloadl`.
    unsafe { store_payload_l(x, payload, true) }
}
crate::ld_c!(i_pl "setpayloadsigl" => __slate_ld_setpayloadsigl);

/// Store [`with_payloadl`]'s NaN in `*x` -- or `+0`, answering `1`.
///
/// # Safety
///
/// `x` must be valid to write and `payload` to read.
#[cfg(target_os = "none")]
unsafe fn store_payload_l(x: *mut L, payload: *const L, signaling: bool) -> i32 {
    // SAFETY: the caller's pointer, and the thunk's pointer to the caller's
    // stack argument.
    let (v, r) = match with_payloadl(unsafe { payload.read() }, signaling) {
        Some(v) => (v, 0),
        None => (L::POS_ZERO, 1),
    };
    // SAFETY: the caller's pointer.
    unsafe { x.write(v) };
    r
}

/// Store `*x` canonicalised in `*cx` and answer `0`; `1`, storing nothing,
/// for an encoding the x87 refuses.
///
/// # Safety
///
/// `x` must be valid to read and `cx` to write, as C requires.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn canonicalizel(cx: *mut L, x: *const L) -> i32 {
    // SAFETY: the caller's pointer.
    match canonical_value_l(unsafe { x.read() }) {
        Some(v) => {
            // SAFETY: the caller's pointer.
            unsafe { cx.write(v) };
            0
        }
        None => 1,
    }
}

/// Whether `*x` is at or below `*y` in IEEE 754's total order.
///
/// # Safety
///
/// Both pointers must be valid to read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn totalorderl(x: *const L, y: *const L) -> i32 {
    // SAFETY: the caller's pointers.
    i32::from(total_order_l(unsafe { x.read() }, unsafe { y.read() }))
}

/// [`totalorderl`] of the magnitudes.
///
/// # Safety
///
/// Both pointers must be valid to read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn totalordermagl(x: *const L, y: *const L) -> i32 {
    // SAFETY: the caller's pointers.
    i32::from(total_order_mag_l(unsafe { x.read() }, unsafe { y.read() }))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines
)]
mod tests {
    use super::*;
    use crate::fenv::{FE_DIVBYZERO, FE_OVERFLOW, FE_UNDERFLOW, feclearexcept, fetestexcept};
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers (`posix/tools/oracle/c23math_harness.py`): one
    /// call a line, `<function> <type> <inputs> = <outputs> <flags> <errno>`.
    const ORACLE: &str = include_str!("c23math_oracle.txt");

    /// C's five exceptions, the ones the oracle reports -- not the x87's
    /// denormal-operand flag, which musl's `FE_ALL_EXCEPT` counts.
    const FIVE: i32 = FE_INVALID | FE_DIVBYZERO | FE_OVERFLOW | FE_UNDERFLOW | FE_INEXACT;

    /// The x87 unit at 64-bit precision, to nearest: what a SlateOS (and
    /// Linux) thread starts with, and not the Windows host's 53 bits.
    fn extended() {
        let cw: u16 = 0x037F;
        // SAFETY: loads the control word from a local.
        unsafe {
            core::arch::asm!("fldcw [{}]", in(reg) &raw const cw, options(nostack, preserves_flags));
        }
    }

    fn begin() {
        feclearexcept(FIVE);
        errno::set_errno(0);
    }

    /// The flags raised since [`begin`], as the oracle writes them.
    fn flags() -> String {
        let f = fetestexcept(FIVE);
        let mut s = String::new();
        for (bit, c) in [
            (FE_INVALID, 'I'),
            (FE_DIVBYZERO, 'Z'),
            (FE_OVERFLOW, 'O'),
            (FE_UNDERFLOW, 'U'),
            (FE_INEXACT, 'X'),
        ] {
            if f & bit != 0 {
                s.push(c);
            }
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    /// `<flags> <errno>`, after a call.
    fn end() -> String {
        format!("{} {}", flags(), errno::get_errno())
    }

    fn f(s: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(s, 16).unwrap())
    }
    fn d(s: &str) -> f64 {
        f64::from_bits(u64::from_str_radix(s, 16).unwrap())
    }
    fn l(s: &str) -> L {
        let (se, m) = s.split_once(':').unwrap();
        L::from_bits(
            u16::from_str_radix(se, 16).unwrap(),
            u64::from_str_radix(m, 16).unwrap(),
        )
    }
    fn hf(x: f32) -> String {
        format!("{:08x}", x.to_bits())
    }
    fn hd(x: f64) -> String {
        format!("{:016x}", x.to_bits())
    }
    fn hl(x: L) -> String {
        format!("{:04x}:{:016x}", x.sign_exp, x.significand)
    }

    /// One `fromfp` line's right-hand side: the five directions of the plain
    /// function, then `/`, then the five of its `x` variant.
    fn fromfp_line(call: &dyn Fn(FromFp, i32) -> String, signed: bool) -> String {
        let (plain, x) = if signed {
            (FROMFP, FROMFPX)
        } else {
            (UFROMFP, UFROMFPX)
        };
        let mut out = Vec::new();
        for kind in [plain, x] {
            if !out.is_empty() {
                out.push(String::from("/"));
            }
            for round in [
                FP_INT_UPWARD,
                FP_INT_DOWNWARD,
                2, // FP_INT_TOWARDZERO
                FP_INT_TONEARESTFROMZERO,
                FP_INT_TONEAREST,
            ] {
                begin();
                let r = call(kind, round);
                out.push(format!("{r}:{}:{}", flags(), errno::get_errno()));
            }
        }
        out.join(" ")
    }

    /// The operation a maximum or minimum function's base name names.
    fn min_max_op(base: &str) -> Option<MinMax> {
        Some(match base {
            "fmaxmag" => MinMax::MaxMag,
            "fminmag" => MinMax::MinMag,
            "fmaximum" => MinMax::Maximum,
            "fminimum" => MinMax::Minimum,
            "fmaximum_num" => MinMax::MaximumNum,
            "fminimum_num" => MinMax::MinimumNum,
            "fmaximum_mag" => MinMax::MaximumMag,
            "fminimum_mag" => MinMax::MinimumMag,
            "fmaximum_mag_num" => MinMax::MaximumMagNum,
            "fminimum_mag_num" => MinMax::MinimumMagNum,
            _ => return None,
        })
    }

    /// Our answer to one oracle line's call, written as the oracle writes
    /// glibc's.
    fn replay(func: &str, t: &str, a: &[&str]) -> String {
        // The function's name without its type suffix (`nextupf` -> `nextup`).
        let base = match t {
            "f" => func.strip_suffix('f').unwrap(),
            "l" => func.strip_suffix('l').unwrap(),
            _ => func,
        };
        if let Some(op) = min_max_op(base) {
            begin();
            let r = match t {
                "f" => hf(min_max(op, f(a[0]), f(a[1]))),
                "d" => hd(min_max(op, d(a[0]), d(a[1]))),
                _ => hl(min_max_l(op, l(a[0]), l(a[1]))),
            };
            return format!("{r} {}", end());
        }
        match base {
            "nextup" | "nextdown" => {
                let up = base == "nextup";
                begin();
                let r = match t {
                    "f" => hf(if up {
                        nextupf(f(a[0]))
                    } else {
                        nextdownf(f(a[0]))
                    }),
                    "d" => hd(if up {
                        nextup(d(a[0]))
                    } else {
                        nextdown(d(a[0]))
                    }),
                    _ => hl(if up {
                        nextupl(l(a[0]))
                    } else {
                        nextdownl(l(a[0]))
                    }),
                };
                format!("{r} {}", end())
            }
            "llogb" => {
                begin();
                let r = match t {
                    "f" => llogbf(f(a[0])),
                    "d" => llogb(d(a[0])),
                    _ => llogbl(l(a[0])),
                };
                format!("{r} {}", end())
            }
            "ilogb" => {
                begin();
                let r = crate::mathl::ilogbl(l(a[0]));
                format!("{r} {}", end())
            }
            "logb" => {
                begin();
                let r = hl(crate::mathl::logbl(l(a[0])));
                format!("{r} {}", end())
            }
            "canonicalize" => {
                begin();
                let r = match t {
                    "f" => {
                        let x = f(a[0]);
                        let mut cx = f32::from_bits(0x1234_5678);
                        // SAFETY: two locals.
                        let r = unsafe { canonicalizef(&raw mut cx, &raw const x) };
                        format!("{r} {}", hf(cx))
                    }
                    "d" => {
                        let x = d(a[0]);
                        let mut cx = f64::from_bits(0x1234_5678_9abc_def0);
                        // SAFETY: two locals.
                        let r = unsafe { canonicalize(&raw mut cx, &raw const x) };
                        format!("{r} {}", hd(cx))
                    }
                    _ => {
                        let x = l(a[0]);
                        let mut cx = L::from_bits(0x1234, 0x1234_5678_9abc_def0);
                        // SAFETY: two locals.
                        let r = unsafe { canonicalizel(&raw mut cx, &raw const x) };
                        format!("{r} {}", hl(cx))
                    }
                };
                format!("{r} {}", end())
            }
            "getpayload" => {
                begin();
                let r = match t {
                    "f" => {
                        let x = f(a[0]);
                        // SAFETY: a local.
                        hf(unsafe { getpayloadf(&raw const x) })
                    }
                    "d" => {
                        let x = d(a[0]);
                        // SAFETY: a local.
                        hd(unsafe { getpayload(&raw const x) })
                    }
                    _ => hl(getpayloadl(l(a[0]))),
                };
                format!("{r} {}", end())
            }
            "setpayload" | "setpayloadsig" => {
                let sig = base == "setpayloadsig";
                begin();
                let r = match t {
                    "f" => {
                        let mut x = 0.0f32;
                        // SAFETY: a local.
                        let r = unsafe {
                            if sig {
                                setpayloadsigf(&raw mut x, f(a[0]))
                            } else {
                                setpayloadf(&raw mut x, f(a[0]))
                            }
                        };
                        format!("{r} {}", hf(x))
                    }
                    "d" => {
                        let mut x = 0.0f64;
                        // SAFETY: a local.
                        let r = unsafe {
                            if sig {
                                setpayloadsig(&raw mut x, d(a[0]))
                            } else {
                                setpayload(&raw mut x, d(a[0]))
                            }
                        };
                        format!("{r} {}", hd(x))
                    }
                    _ => {
                        let (v, r) = match with_payloadl(l(a[0]), sig) {
                            Some(v) => (v, 0),
                            None => (L::POS_ZERO, 1),
                        };
                        format!("{r} {}", hl(v))
                    }
                };
                format!("{r} {}", end())
            }
            "totalorder" | "totalordermag" => {
                let mag = base == "totalordermag";
                begin();
                let r = match t {
                    "f" => {
                        let (x, y) = (f(a[0]), f(a[1]));
                        // SAFETY: two locals.
                        unsafe {
                            if mag {
                                totalordermagf(&raw const x, &raw const y)
                            } else {
                                totalorderf(&raw const x, &raw const y)
                            }
                        }
                    }
                    "d" => {
                        let (x, y) = (d(a[0]), d(a[1]));
                        // SAFETY: two locals.
                        unsafe {
                            if mag {
                                totalordermag(&raw const x, &raw const y)
                            } else {
                                totalorder(&raw const x, &raw const y)
                            }
                        }
                    }
                    _ => {
                        let (x, y) = (l(a[0]), l(a[1]));
                        i32::from(if mag {
                            total_order_mag_l(x, y)
                        } else {
                            total_order_l(x, y)
                        })
                    }
                };
                format!("{r} {}", end())
            }
            "fromfp" | "ufromfp" => {
                let signed = base == "fromfp";
                let width: u32 = a[1].parse().unwrap();
                let call = |kind: FromFp, round: i32| -> String {
                    let bits = match t {
                        "f" => kind.binary(f(a[0]), round, width),
                        "d" => kind.binary(d(a[0]), round, width),
                        _ => kind.extended(l(a[0]), round, width),
                    };
                    if signed {
                        format!("{}", bits as i64)
                    } else {
                        format!("{bits}")
                    }
                };
                fromfp_line(&call, signed)
            }
            "scalb" => {
                begin();
                let r = hl(scalbl(l(a[0]), l(a[1])));
                format!("{r} {}", end())
            }
            other => panic!("no replay for {other}"),
        }
    }

    /// Every call the oracle made, answered as glibc answered it: the value,
    /// every exception raised, and `errno`.
    #[test]
    fn every_answer_is_glibcs() {
        extended();
        let mut failures = Vec::new();
        let mut calls = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
        {
            let (lhs, glibc) = line.split_once(" = ").unwrap();
            let mut words = lhs.split(' ');
            let func = words.next().unwrap();
            let t = words.next().unwrap();
            let args: Vec<&str> = words.collect();
            let ours = replay(func, t, &args);
            calls += 1;
            if ours != glibc {
                failures.push(format!("{lhs}\n    glibc {glibc}\n    ours  {ours}"));
            }
        }
        assert!(calls > 10_000, "the oracle is whole: {calls} calls");
        let shown = failures.len().min(60);
        assert!(
            failures.is_empty(),
            "{} of {calls} differ; the first {shown}:\n{}",
            failures.len(),
            failures[..shown].join("\n")
        );
    }

    /// The C names by their types, called as C calls them: `nextup` past the
    /// largest finite value, `fromfp`'s saturation, a payload round trip.
    #[test]
    fn the_c_entry_points_answer() {
        extended();
        assert_eq!(nextup(f64::MAX), f64::INFINITY);
        assert_eq!(nextdown(0.0).to_bits(), (-f64::from_bits(1)).to_bits());
        assert_eq!(nextupf(-0.0).to_bits(), 1);
        errno::set_errno(0);
        assert_eq!(fromfp(1e30, FP_INT_TONEAREST, 32), i64::from(i32::MAX));
        assert_eq!(errno::get_errno(), errno::EDOM);
        assert_eq!(ufromfpx(2.5, FP_INT_TONEAREST, 8), 2);
        let mut nan = 0.0f64;
        // SAFETY: locals.
        unsafe {
            assert_eq!(setpayload(&raw mut nan, 1234.0), 0);
            assert_eq!(getpayload(&raw const nan), 1234.0);
            assert_eq!(setpayloadsig(&raw mut nan, 0.0), 1);
            assert_eq!(nan.to_bits(), 0);
        }
        assert_eq!(fmaximum(-0.0, 0.0).to_bits(), 0);
        assert_eq!(fminimum_num(f64::NAN, 3.0), 3.0);
        assert!(fmaximum(f64::NAN, 3.0).is_nan());
        let (a, b) = (-f64::NAN, f64::INFINITY);
        // SAFETY: locals.
        assert_eq!(unsafe { totalorder(&raw const a, &raw const b) }, 1);
    }
}

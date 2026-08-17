//! The x87 80-bit extended-precision float, in software: `long double` as GNU's
//! numeric utilities actually compute with it.
//!
//! # Why this exists
//!
//! `seq`, `printf`, `expr` and `bc` all read their operands with `strtold` and
//! write them with `printf`'s `%L` conversions, which on x86-64 means a **64-bit
//! significand**, not the 53 bits of a `double`. Rust has no `f80`, so a port
//! that reaches for `f64` is not making a small approximation — it is answering
//! a different question. Measured against GNU `seq` 9.4 over 4000 random
//! three-operand ranges with 10 to 20 decimal places, an `f64` implementation
//! disagreed on **1355** of them, and the disagreement is visible on the very
//! first line, before any arithmetic happens:
//!
//! ```text
//! $ seq 145.0612310077283783 237.4095461113930955 266.1663049403269910
//! 145.0612310077283783        <- GNU
//! 145.0612310077283666        <- the same program built on f64
//! ```
//!
//! That is `strtold` and `%.16Lf` alone. No amount of care in the loop recovers
//! it, because the digits were already gone.
//!
//! # Why software rather than the hardware that is right there
//!
//! Every machine this will run on has an x87 unit, and Rust can reach it with
//! inline assembly. It is not used, for three reasons:
//!
//! - **The precision is a mode, not a property.** x87 rounds to whatever the
//!   control word says, and Windows — where this crate's tests run — sets it to
//!   *53* bits, not 64. Hardware arithmetic would therefore be silently wrong
//!   on the host and right on the target, which is the worst of the two.
//! - **It would not answer the hard half.** The divergence above is in decimal
//!   conversion, and there is no x87 instruction for `strtold` or `%.16Lf`. The
//!   exact big-integer machinery in [`crate::bignat`] is needed either way.
//! - **It cannot be tested.** A soft implementation can be checked digit by
//!   digit against glibc from a unit test; an `asm!` block can only be checked
//!   by running it on the hardware whose mode is the thing in doubt.
//!
//! # What is faithful, and what is not
//!
//! Arithmetic is round-to-nearest-even at 64 bits of significand, with
//! subnormals — the x87 default control word. Decimal conversion is exact in
//! both directions: the parser divides big integers and keeps the remainder as
//! a real sticky bit, and the formatter expands `m * 2^e` into its full decimal
//! form, which is always finite, and rounds that. `printf`'s ties therefore
//! break to even against the true value rather than against an approximation.
//!
//! One measured divergence is deliberate and documented rather than fixed:
//! glibc sets `ERANGE` for a **hexadecimal** literal that is subnormal *and*
//! inexact (`0x3p-16446`) but not for one that is merely subnormal
//! (`0x1p-16444`) or even subnormal and inexact by many bits
//! (`0x1.0000000000000001p-16400`). No rule fits all three; this module flags
//! a hex literal only when it overflows to infinity or flushes a nonzero value
//! to zero. For **decimal** literals the rule is glibc's own and exact: any
//! result that lands in the subnormal range is a range error, which is why GNU
//! `seq 1e-4932` refuses its operand.

use crate::bignat::Nat;
use std::cell::RefCell;
use std::cmp::Ordering;

/// The exponent bias of the 80-bit format.
const BIAS: i32 = 16383;
/// The largest biased exponent that still denotes a finite number.
const MAX_BIASED: u16 = 0x7ffe;
/// The biased exponent of infinities and NaNs.
const INF_BIASED: u16 = 0x7fff;
/// The value of `e` in `m * 2^e` for every subnormal: one below the smallest
/// normal exponent, less the 63 bits of fraction.
const SUBNORMAL_EXP: i32 = 1 - BIAS - 63;

/// An x87 80-bit extended-precision float.
///
/// Stored as the hardware stores it — sign, 15-bit biased exponent, and a
/// **64-bit significand whose integer bit is explicit**. That last part is why
/// `%La` prints `1.0` as `0x8p-3` rather than `0x1p+0`: the leading hex digit
/// is the top four bits of a 64-bit field, not a normalised `1.`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExtF80 {
    neg: bool,
    /// Biased exponent. 0 is zero-or-subnormal, `0x7fff` is infinity-or-NaN.
    exp: u16,
    /// The significand, integer bit at bit 63.
    sig: u64,
}

impl ExtF80 {
    /// Positive zero.
    pub const ZERO: ExtF80 = ExtF80 {
        neg: false,
        exp: 0,
        sig: 0,
    };
    /// One.
    pub const ONE: ExtF80 = ExtF80 {
        neg: false,
        exp: BIAS as u16,
        sig: 1 << 63,
    };
    /// Positive infinity.
    pub const INFINITY: ExtF80 = ExtF80 {
        neg: false,
        exp: INF_BIASED,
        sig: 1 << 63,
    };
    /// A quiet NaN.
    pub const NAN: ExtF80 = ExtF80 {
        neg: false,
        exp: INF_BIASED,
        sig: 0xc000_0000_0000_0000,
    };

    /// Whether the sign bit is set. True for `-0.0`, which is why this is not
    /// spelled `is_negative`.
    pub fn sign_bit(self) -> bool {
        self.neg
    }

    /// Whether this is a NaN.
    pub fn is_nan(self) -> bool {
        self.exp == INF_BIASED && self.sig != 1 << 63
    }

    /// Whether this is an infinity.
    pub fn is_infinite(self) -> bool {
        self.exp == INF_BIASED && self.sig == 1 << 63
    }

    /// Whether this is finite — neither infinite nor NaN.
    pub fn is_finite(self) -> bool {
        self.exp != INF_BIASED
    }

    /// Whether this is zero, of either sign.
    pub fn is_zero(self) -> bool {
        self.exp == 0 && self.sig == 0
    }

    /// Whether this is a nonzero number below the smallest normal.
    pub fn is_subnormal(self) -> bool {
        self.exp == 0 && self.sig != 0
    }

    /// A small non-negative integer, exactly.
    pub fn from_u32(v: u32) -> Self {
        if v == 0 {
            return ExtF80::ZERO;
        }
        round(false, u128::from(v), 0, false)
    }

    /// `-self`. The sign bit flips even for zero and NaN.
    pub fn neg(self) -> Self {
        ExtF80 {
            neg: !self.neg,
            ..self
        }
    }

    /// The value as `m * 2^e` with `m` a 64-bit integer. Meaningless for
    /// infinities and NaNs, which callers exclude first.
    fn decompose(self) -> (u64, i32) {
        if self.exp == 0 {
            (self.sig, SUBNORMAL_EXP)
        } else {
            (self.sig, i32::from(self.exp) - BIAS - 63)
        }
    }

    /// `self * other`, rounded to nearest with ties to even.
    #[must_use]
    pub fn mul(self, other: Self) -> Self {
        if self.is_nan() || other.is_nan() {
            return ExtF80::NAN;
        }
        let neg = self.neg ^ other.neg;
        if self.is_infinite() || other.is_infinite() {
            // Infinity times zero is the one product with no limit.
            if self.is_zero() || other.is_zero() {
                return ExtF80::NAN;
            }
            return ExtF80 {
                neg,
                ..ExtF80::INFINITY
            };
        }
        if self.is_zero() || other.is_zero() {
            return ExtF80 {
                neg,
                ..ExtF80::ZERO
            };
        }
        let (ma, ea) = self.decompose();
        let (mb, eb) = other.decompose();
        round(neg, u128::from(ma) * u128::from(mb), ea + eb, false)
    }

    /// `self + other`, rounded to nearest with ties to even.
    #[must_use]
    #[allow(clippy::similar_names)]
    pub fn add(self, other: Self) -> Self {
        if self.is_nan() || other.is_nan() {
            return ExtF80::NAN;
        }
        if self.is_infinite() || other.is_infinite() {
            if self.is_infinite() && other.is_infinite() && self.neg != other.neg {
                return ExtF80::NAN;
            }
            return if self.is_infinite() { self } else { other };
        }
        if self.is_zero() && other.is_zero() {
            // Two zeros keep a sign only when they agree; round-to-nearest
            // makes `(+0) + (-0)` positive.
            return ExtF80 {
                neg: self.neg && other.neg,
                ..ExtF80::ZERO
            };
        }
        if self.is_zero() {
            return other;
        }
        if other.is_zero() {
            return self;
        }

        let (ma, ea) = self.decompose();
        let (mb, eb) = other.decompose();
        let (big_neg, big_m, big_e, small_neg, small_m, small_e) = if ea >= eb {
            (self.neg, ma, ea, other.neg, mb, eb)
        } else {
            (other.neg, mb, eb, self.neg, ma, ea)
        };

        // Line both up 63 bits below the larger one's own exponent. That leaves
        // the larger operand at most 127 bits wide, so the sum still fits, and
        // it gives 63 bits of room under the smaller one before anything has to
        // be summarised into a sticky bit.
        let target = big_e - 63;
        let delta = big_e.saturating_sub(small_e);
        let a = u128::from(big_m) << 63;
        let (b, sticky) = if delta <= 63 {
            #[allow(clippy::cast_sign_loss)]
            (u128::from(small_m) << (63 - delta) as u32, false)
        } else if delta < 63 + 128 {
            #[allow(clippy::cast_sign_loss)]
            let drop = (delta - 63) as u32;
            let whole = u128::from(small_m);
            (whole >> drop, whole & ((1u128 << drop) - 1) != 0)
        } else {
            (0, true)
        };

        if big_neg == small_neg {
            return round(big_neg, a + b, target, sticky);
        }
        match a.cmp(&b) {
            // Exact cancellation. The sign is *not* either operand's: a sum
            // that is exactly zero is positive under round-to-nearest, which is
            // the only rounding mode we implement. (`sticky` cannot hold here.
            // It is set only when the smaller operand sits more than 63 bits
            // below the larger, and a normal larger operand then has
            // `a >= 2^126 > b`, while a subnormal larger operand forces the
            // smaller to share its exponent and set no sticky bit at all. So
            // equality really does mean the difference is zero.)
            Ordering::Equal => ExtF80::ZERO,
            Ordering::Greater => {
                // Whenever bits were summarised away, the smaller operand is
                // truly a little larger than `b`, so the difference is a little
                // smaller than `a - b`: step down one and let the sticky bit
                // say the true value lies between. `a - b` is at least 2^62
                // whenever `sticky` holds, so the step cannot underflow.
                let mut diff = a - b;
                if sticky {
                    diff -= 1;
                }
                round(big_neg, diff, target, sticky)
            }
            Ordering::Less => round(small_neg, b - a, target, false),
        }
    }

    /// Ordering by value; `None` when either side is NaN. Zeros of opposite
    /// sign compare equal, as they must.
    pub fn partial_cmp(self, other: Self) -> Option<Ordering> {
        if self.is_nan() || other.is_nan() {
            return None;
        }
        if self.is_zero() && other.is_zero() {
            return Some(Ordering::Equal);
        }
        let key = |v: ExtF80| (u128::from(v.exp) << 64) | u128::from(v.sig);
        Some(match (self.neg, other.neg) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => key(self).cmp(&key(other)),
            (true, true) => key(other).cmp(&key(self)),
        })
    }

    /// `self < other`, false if either is NaN.
    pub fn lt(self, other: Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Less)
    }

    /// `self == other` by value, false if either is NaN.
    pub fn eq_value(self, other: Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

/// Assemble `sig * 2^exp2`, plus a positive infinitesimal when `sticky`, into
/// the nearest representable value, ties to even.
///
/// This is the only place a value is rounded, so it is the only place the
/// subnormal and overflow boundaries are decided.
fn round(neg: bool, sig: u128, exp2: i32, sticky: bool) -> ExtF80 {
    if sig == 0 {
        // A pure sticky bit is an infinitesimal, which rounds to zero.
        return ExtF80 {
            neg,
            ..ExtF80::ZERO
        };
    }
    let bits = 128 - sig.leading_zeros() as i32;
    // The shift that would leave a 64-bit significand...
    let normal_shift = bits - 64;
    // ...unless that lands below the subnormal floor, where the exponent is
    // pinned and precision is given up instead.
    let shift = if exp2 + normal_shift < SUBNORMAL_EXP {
        SUBNORMAL_EXP - exp2
    } else {
        normal_shift
    };

    let (mut m, guard, mut sticky) = if shift > 0 {
        #[allow(clippy::cast_sign_loss)]
        let s = shift as u32;
        if s >= 128 {
            // Everything, including the rounding bit, is below the floor.
            return ExtF80 {
                neg,
                ..ExtF80::ZERO
            };
        }
        let guard = (sig >> (s - 1)) & 1 == 1;
        let below = s - 1;
        let lost = below > 0 && sig & ((1u128 << below) - 1) != 0;
        (sig >> s, guard, sticky || lost)
    } else {
        #[allow(clippy::cast_sign_loss)]
        (sig << (-shift) as u32, false, sticky)
    };
    let mut e = exp2 + shift;

    if guard && (sticky || m & 1 == 1) {
        m += 1;
    }
    sticky = false;
    let _ = sticky;
    if m >> 64 != 0 {
        m >>= 1;
        e += 1;
    }

    if m == 0 {
        return ExtF80 {
            neg,
            ..ExtF80::ZERO
        };
    }
    #[allow(clippy::cast_possible_truncation)]
    let m = m as u64;
    if m >> 63 == 0 {
        // Still under the integer bit, so this is a subnormal and `e` is pinned
        // at the floor by construction above.
        debug_assert_eq!(e, SUBNORMAL_EXP);
        return ExtF80 {
            neg,
            exp: 0,
            sig: m,
        };
    }
    let biased = e + 63 + BIAS;
    if biased > i32::from(MAX_BIASED) {
        return ExtF80 {
            neg,
            ..ExtF80::INFINITY
        };
    }
    debug_assert!(biased >= 1);
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    ExtF80 {
        neg,
        exp: biased as u16,
        sig: m,
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// What one `strtold` call did: the value, how much of the input it claimed,
/// and whether it would have set `ERANGE`.
#[derive(Clone, Copy, Debug)]
pub struct Scanned {
    pub value: ExtF80,
    /// Bytes consumed. Zero means "no conversion could be performed", which is
    /// `strtold` returning `endptr == nptr`.
    pub consumed: usize,
    /// glibc's `ERANGE`. See the module documentation for the one hexadecimal
    /// case where this deliberately differs.
    pub range_error: bool,
}

/// Any decimal exponent past this cannot change the answer: the format's range
/// is `10^-4951` to `10^4932`, so a value beyond these bounds is an infinity or
/// a zero whatever its digits are. The guard exists so that a numeral like
/// `1e-99999999999999999999` is answered immediately rather than by building
/// `10^99999999999999999999`.
const EXPONENT_GUARD: i64 = 6000;

/// C's `strtold` in the C locale, as glibc implements it.
///
/// Accepts leading whitespace, an optional sign, then a decimal numeral with an
/// optional `e` exponent, a `0x` numeral with an optional `p` exponent, `inf`
/// or `infinity`, or `nan` with an optional parenthesised payload — each of the
/// words in any case.
pub fn strtold(s: &[u8]) -> Scanned {
    let mut i = 0;
    while matches!(s.get(i), Some(c) if c.is_ascii_whitespace() || *c == 0x0b) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };

    if let Some(scanned) = scan_word(s, i, neg) {
        return scanned;
    }
    if matches!(s.get(i), Some(b'0'))
        && matches!(s.get(i + 1), Some(b'x' | b'X'))
        && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit)
    {
        return scan_hex(s, i + 2, neg);
    }
    if matches!(s.get(i), Some(b'0'))
        && matches!(s.get(i + 1), Some(b'x' | b'X'))
        && matches!(s.get(i + 2), Some(b'.'))
        && s.get(i + 3).is_some_and(u8::is_ascii_hexdigit)
    {
        return scan_hex(s, i + 2, neg);
    }
    scan_decimal(s, i, neg)
}

/// `inf`, `infinity` and `nan`, which are words rather than numerals.
fn scan_word(s: &[u8], at: usize, neg: bool) -> Option<Scanned> {
    let starts = |word: &[u8]| {
        s.get(at..at + word.len())
            .is_some_and(|got| got.eq_ignore_ascii_case(word))
    };
    if starts(b"infinity") {
        return Some(Scanned {
            value: ExtF80 {
                neg,
                ..ExtF80::INFINITY
            },
            consumed: at + 8,
            range_error: false,
        });
    }
    if starts(b"inf") {
        return Some(Scanned {
            value: ExtF80 {
                neg,
                ..ExtF80::INFINITY
            },
            consumed: at + 3,
            range_error: false,
        });
    }
    if starts(b"nan") {
        let mut end = at + 3;
        // An `n-char-sequence` in parentheses is part of the token, but only if
        // it is closed; an unclosed one leaves the token at `nan`.
        if matches!(s.get(end), Some(b'(')) {
            let mut j = end + 1;
            while matches!(s.get(j), Some(c) if c.is_ascii_alphanumeric() || *c == b'_') {
                j += 1;
            }
            if matches!(s.get(j), Some(b')')) {
                end = j + 1;
            }
        }
        return Some(Scanned {
            value: ExtF80 { neg, ..ExtF80::NAN },
            consumed: end,
            range_error: false,
        });
    }
    None
}

/// Read the digits of an exponent, saturating rather than wrapping: a numeral
/// may spell an exponent that does not fit any integer type.
fn scan_exponent(s: &[u8], at: usize, marks: &[u8]) -> Option<(i64, usize)> {
    if !matches!(s.get(at), Some(c) if marks.contains(c)) {
        return None;
    }
    let mut j = at + 1;
    let neg = match s.get(j) {
        Some(b'-') => {
            j += 1;
            true
        }
        Some(b'+') => {
            j += 1;
            false
        }
        _ => false,
    };
    let start = j;
    let mut value: i64 = 0;
    while let Some(&c) = s.get(j) {
        if !c.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(c - b'0'))
            .min(i64::MAX / 16);
        j += 1;
    }
    if j == start {
        // `1e` and `1e+` are the numeral `1` followed by junk.
        return None;
    }
    Some((if neg { -value } else { value }, j))
}

/// The decimal numeral path.
fn scan_decimal(s: &[u8], at: usize, neg: bool) -> Scanned {
    let mut digits: Vec<u8> = Vec::new();
    let mut i = at;
    let mut seen = false;
    while let Some(&c) = s.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        digits.push(c);
        seen = true;
        i += 1;
    }
    let mut frac_len: i64 = 0;
    if matches!(s.get(i), Some(b'.')) {
        let mut j = i + 1;
        while let Some(&c) = s.get(j) {
            if !c.is_ascii_digit() {
                break;
            }
            digits.push(c);
            seen = true;
            frac_len += 1;
            j += 1;
        }
        // A lone `.` with no digits either side is not a numeral at all; with
        // digits before it, it ends the numeral.
        if seen {
            i = j;
        }
    }
    if !seen {
        return Scanned {
            value: ExtF80::ZERO,
            consumed: 0,
            range_error: false,
        };
    }
    let mut exp10: i64 = -frac_len;
    if let Some((written, end)) = scan_exponent(s, i, b"eE") {
        exp10 = exp10.saturating_add(written);
        i = end;
    }

    let (value, range_error) = decimal_value(neg, &digits, exp10);
    Scanned {
        value,
        consumed: i,
        range_error,
    }
}

/// Turn the collected digits and a power of ten into the nearest value, and say
/// whether glibc would call it a range error.
///
/// The decimal rule is glibc's: **any** result in the subnormal range is a
/// range error, whether or not a bit was lost. That is why `seq 1e-4932`, a
/// perfectly ordinary-looking operand, is refused.
fn decimal_value(neg: bool, digits: &[u8], exp10: i64) -> (ExtF80, bool) {
    let lead = digits.iter().position(|&c| c != b'0');
    let Some(lead) = lead else {
        // All zeros: exactly zero, whatever the exponent says.
        return (
            ExtF80 {
                neg,
                ..ExtF80::ZERO
            },
            false,
        );
    };
    let mut significant = &digits[lead..];
    let mut exp10 = exp10;
    // Trailing zeros are exponent, not precision, and dropping them here keeps
    // the big integers as small as the numeral allows.
    while significant.len() > 1 && significant.last() == Some(&b'0') {
        significant = &significant[..significant.len() - 1];
        exp10 = exp10.saturating_add(1);
    }

    let magnitude = exp10.saturating_add(significant.len() as i64);
    if magnitude > EXPONENT_GUARD {
        return (
            ExtF80 {
                neg,
                ..ExtF80::INFINITY
            },
            true,
        );
    }
    if magnitude < -EXPONENT_GUARD {
        return (
            ExtF80 {
                neg,
                ..ExtF80::ZERO
            },
            true,
        );
    }

    let d = Nat::from_decimal(significant);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (num, den) = if exp10 >= 0 {
        (d.mul(&Nat::pow(10, exp10 as u32)), Nat::from_u64(1))
    } else {
        (d, Nat::pow(10, (-exp10) as u32))
    };
    let value = quotient_to_float(neg, &num, &den);

    let overflowed = value.is_infinite();
    let flushed = value.is_zero();
    (value, overflowed || flushed || value.is_subnormal())
}

/// Round `num / den` — both exact, `den` nonzero — to the nearest value.
///
/// The numerator is shifted so the quotient is 65 bits before dividing, which
/// is what keeps the cost linear: algorithm D pays per quotient limb, and three
/// limbs is all that is ever asked for. The remainder then *is* the sticky bit.
fn quotient_to_float(neg: bool, num: &Nat, den: &Nat) -> ExtF80 {
    if num.is_zero() {
        return ExtF80 {
            neg,
            ..ExtF80::ZERO
        };
    }
    let want = 65_i64;
    let shift = want + den.bit_len() as i64 - num.bit_len() as i64;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let (n2, d2) = if shift >= 0 {
        (num.shl(shift as usize), den.clone())
    } else {
        (num.clone(), den.shl((-shift) as usize))
    };
    let (q, r) = n2.divmod(&d2);
    let sticky = !r.is_zero();
    // The quotient is 64 to 66 bits by construction, so it fits a `u128`.
    let mut sig: u128 = 0;
    for i in (0..q.bit_len()).rev() {
        sig = (sig << 1) | u128::from(q.bit(i));
    }
    #[allow(clippy::cast_possible_truncation)]
    round(neg, sig, -(shift as i32), sticky)
}

/// The `0x` numeral path. `at` points just past the `0x`.
fn scan_hex(s: &[u8], at: usize, neg: bool) -> Scanned {
    let mut digits: Vec<u8> = Vec::new();
    let mut i = at;
    while let Some(&c) = s.get(i) {
        if !c.is_ascii_hexdigit() {
            break;
        }
        digits.push(c);
        i += 1;
    }
    let mut frac_len: i64 = 0;
    if matches!(s.get(i), Some(b'.')) {
        let mut j = i + 1;
        while let Some(&c) = s.get(j) {
            if !c.is_ascii_hexdigit() {
                break;
            }
            digits.push(c);
            frac_len += 1;
            j += 1;
        }
        i = j;
    }
    let mut exp2: i64 = -4 * frac_len;
    if let Some((written, end)) = scan_exponent(s, i, b"pP") {
        exp2 = exp2.saturating_add(written);
        i = end;
    }

    if digits.iter().all(|&c| c == b'0') {
        return Scanned {
            value: ExtF80 {
                neg,
                ..ExtF80::ZERO
            },
            consumed: i,
            range_error: false,
        };
    }
    let d = Nat::from_hex(&digits);
    // Keep 65 bits and a sticky bit; `round` wants a guard bit to work with.
    let bits = d.bit_len();
    let (sig_nat, exp2, sticky) = if bits > 65 {
        let drop = bits - 65;
        (
            d.shr(drop),
            exp2.saturating_add(drop as i64),
            d.any_below(drop),
        )
    } else {
        (d, exp2, false)
    };
    let mut sig: u128 = 0;
    for i in (0..sig_nat.bit_len()).rev() {
        sig = (sig << 1) | u128::from(sig_nat.bit(i));
    }
    let clamped = exp2.clamp(-EXPONENT_GUARD * 8, EXPONENT_GUARD * 8);
    #[allow(clippy::cast_possible_truncation)]
    let value = round(neg, sig, clamped as i32, sticky);
    // See the module documentation: a hex literal is flagged only when it left
    // the representable range entirely, not merely when it landed subnormal.
    let range_error = value.is_infinite() || value.is_zero();
    Scanned {
        value,
        consumed: i,
        range_error,
    }
}

/// gnulib's `xstrtold` with a null `ptr`: the whole string must be one numeral,
/// and a range error is fatal unless the value came out zero.
///
/// The zero exception is not an oversight upstream — an underflow that reaches
/// zero is a normal answer, while one that stops at a subnormal is not, and
/// that is exactly the distinction `val != 0 && errno == ERANGE` draws.
pub fn xstrtold(s: &[u8]) -> Option<ExtF80> {
    let got = strtold(s);
    if got.consumed == 0 || got.consumed != s.len() {
        return None;
    }
    if got.range_error && !got.value.is_zero() {
        return None;
    }
    Some(got.value)
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// One `printf` conversion for a `long double` argument.
///
/// Built by the caller rather than parsed here, because the utilities that need
/// it also need to *reject* the spellings they do not implement, with their own
/// wording; `seq -f %d` is `seq`'s diagnostic, not this module's.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// `-`: pad on the right instead of the left.
    pub minus: bool,
    /// `+`: always write a sign.
    pub plus: bool,
    /// ` `: write a space where a `+` would go.
    pub space: bool,
    /// `#`: keep the radix point even when nothing follows it, and keep `%g`'s
    /// trailing zeros.
    pub hash: bool,
    /// `0`: pad with zeros between the sign and the digits.
    pub zero: bool,
    /// The minimum field width.
    pub width: usize,
    /// The precision, if one was written. Its meaning depends on `conv`.
    pub precision: Option<usize>,
    /// One of `efgaEFGA`.
    pub conv: u8,
}

impl Spec {
    /// The plain `%.<prec>Lf` that `seq` builds when every operand is a fixed
    /// point decimal.
    pub fn fixed(precision: usize) -> Self {
        Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: Some(precision),
            conv: b'f',
        }
    }

    /// The zero-padded `%0<width>.<prec>Lf` that `seq -w` builds.
    pub fn zero_padded(width: usize, precision: usize) -> Self {
        Spec {
            zero: true,
            width,
            ..Spec::fixed(precision)
        }
    }

    /// The bare `%Lg` that `seq` falls back to.
    pub fn general() -> Self {
        Spec {
            precision: None,
            conv: b'g',
            ..Spec::fixed(0)
        }
    }
}

/// Format one value, as glibc's `printf` would in the C locale.
#[must_use]
pub fn render(spec: &Spec, v: ExtF80) -> String {
    let upper = spec.conv.is_ascii_uppercase();
    let conv = spec.conv.to_ascii_lowercase();
    let sign = if v.sign_bit() {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };

    if !v.is_finite() {
        let word = if v.is_nan() { "nan" } else { "inf" };
        let word = if upper {
            word.to_ascii_uppercase()
        } else {
            word.to_string()
        };
        // The zero flag never applies to these: there are no digits to pad.
        return place(spec, sign, "", &word, false);
    }

    let (prefix, body) = match conv {
        b'f' => (
            String::new(),
            fixed_form(v, spec.precision.unwrap_or(6), spec.hash),
        ),
        b'e' => (
            String::new(),
            scientific_form(v, spec.precision.unwrap_or(6), spec.hash, upper),
        ),
        b'g' => (
            String::new(),
            general_form(v, spec.precision, spec.hash, upper),
        ),
        _ => hex_form(v, spec.precision, spec.hash, upper),
    };
    place(spec, sign, &prefix, &body, spec.zero)
}

/// Lay a formatted number into its field.
fn place(spec: &Spec, sign: &str, prefix: &str, body: &str, zero_pad: bool) -> String {
    let used = sign.len() + prefix.len() + body.len();
    let fill = spec.width.saturating_sub(used);
    if fill == 0 {
        return format!("{sign}{prefix}{body}");
    }
    if spec.minus {
        format!("{sign}{prefix}{body}{}", " ".repeat(fill))
    } else if zero_pad {
        // Zeros go inside: after the sign and after `%a`'s `0x`, never before.
        format!("{sign}{prefix}{}{body}", "0".repeat(fill))
    } else {
        format!("{}{sign}{prefix}{body}", " ".repeat(fill))
    }
}

thread_local! {
    /// One `seq` run formats thousands of numbers whose binary exponents barely
    /// move, and `5^k` is the only expensive part of expanding one. Keeping the
    /// last one turns a per-number big multiply into a per-run one.
    static LAST_POW5: RefCell<(u32, Nat)> = RefCell::new((0, Nat::from_u64(1)));
}

/// `5^k`, remembering the most recent.
fn pow5(k: u32) -> Nat {
    LAST_POW5.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.0 != k {
            *slot = (k, Nat::pow(5, k));
        }
        slot.1.clone()
    })
}

/// The exact decimal expansion of a finite value: the integer digits, and the
/// fraction digits.
///
/// It is always finite. A binary fraction `m / 2^k` is `m * 5^k / 10^k`, so it
/// terminates after exactly `k` decimal places — which is why a formatter can
/// round the true value rather than an approximation of it.
fn expand(v: ExtF80) -> (Vec<u8>, Vec<u8>) {
    let (m, e) = v.decompose();
    if m == 0 {
        return (vec![b'0'], Vec::new());
    }
    let n = Nat::from_u64(m);
    if e >= 0 {
        #[allow(clippy::cast_sign_loss)]
        return (n.shl(e as usize).to_decimal(), Vec::new());
    }
    #[allow(clippy::cast_sign_loss)]
    let k = (-e) as usize;
    let int_part = n.shr(k);
    let frac_part = n.sub(&int_part.shl(k));
    let mut frac = frac_part
        .mul(&pow5(u32::try_from(k).unwrap_or(u32::MAX)))
        .to_decimal();
    if frac_part.is_zero() {
        frac.clear();
    }
    // The fraction occupies exactly `k` places; `to_decimal` dropped the
    // leading zeros that say where it starts.
    let mut padded = vec![b'0'; k.saturating_sub(frac.len())];
    padded.extend_from_slice(&frac);
    (int_part.to_decimal(), padded)
}

/// Round a digit string to `keep` digits, half to even against the exact tail.
///
/// Returns the kept digits and whether the rounding carried past the front, in
/// which case the caller gains a digit and, for a scientific form, an exponent.
fn round_digits(digits: &[u8], keep: usize) -> (Vec<u8>, bool) {
    if keep >= digits.len() {
        let mut out = digits.to_vec();
        out.resize(keep, b'0');
        return (out, false);
    }
    let mut out = digits[..keep].to_vec();
    let first_dropped = digits[keep];
    let tail_nonzero = digits[keep + 1..].iter().any(|&c| c != b'0');
    let last_kept_odd = out.last().is_some_and(|c| (c - b'0') % 2 == 1);
    let up = match first_dropped.cmp(&b'5') {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => tail_nonzero || last_kept_odd,
    };
    if !up {
        return (out, false);
    }
    for i in (0..out.len()).rev() {
        if out[i] == b'9' {
            out[i] = b'0';
        } else {
            out[i] += 1;
            return (out, false);
        }
    }
    // Every digit was a nine: the value became a power of ten.
    (out, true)
}

/// `%f`.
fn fixed_form(v: ExtF80, precision: usize, hash: bool) -> String {
    let (int_digits, frac_digits) = expand(v);
    let mut all = int_digits.clone();
    all.extend_from_slice(&frac_digits);
    let keep = int_digits.len() + precision;
    let (kept, carried) = round_digits(&all, keep);
    let mut kept = kept;
    if carried {
        kept.insert(0, b'1');
    }
    let int_len = int_digits.len() + usize::from(carried);
    let (int_part, frac_part) = kept.split_at(int_len);
    let int_text = String::from_utf8_lossy(int_part).into_owned();
    if precision == 0 && !hash {
        return int_text;
    }
    format!("{int_text}.{}", String::from_utf8_lossy(frac_part))
}

/// The significant digits and the power of ten of the first one. A zero value
/// has no significant digits.
fn significant(v: ExtF80) -> (Vec<u8>, i32) {
    let (int_digits, frac_digits) = expand(v);
    let mut all = int_digits.clone();
    all.extend_from_slice(&frac_digits);
    let Some(lead) = all.iter().position(|&c| c != b'0') else {
        return (Vec::new(), 0);
    };
    let exp10 = i32::try_from(int_digits.len()).unwrap_or(i32::MAX)
        - 1
        - i32::try_from(lead).unwrap_or(i32::MAX);
    let mut digits = all[lead..].to_vec();
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    (digits, exp10)
}

/// `%e`.
fn scientific_form(v: ExtF80, precision: usize, hash: bool, upper: bool) -> String {
    let (digits, exp10) = significant(v);
    let (kept, exp10) = if digits.is_empty() {
        (vec![b'0'; precision + 1], 0)
    } else {
        let (mut kept, carried) = round_digits(&digits, precision + 1);
        let exp10 = if carried {
            kept.insert(0, b'1');
            kept.truncate(precision + 1);
            exp10 + 1
        } else {
            exp10
        };
        (kept, exp10)
    };
    let mark = if upper { 'E' } else { 'e' };
    let head = char::from(kept.first().copied().unwrap_or(b'0'));
    let tail = String::from_utf8_lossy(kept.get(1..).unwrap_or_default()).into_owned();
    let point = if precision > 0 || hash { "." } else { "" };
    let esign = if exp10 < 0 { '-' } else { '+' };
    format!("{head}{point}{tail}{mark}{esign}{:02}", exp10.abs())
}

/// `%g` — `%e` or `%f` depending on the exponent, then trailing zeros removed.
fn general_form(v: ExtF80, precision: Option<usize>, hash: bool, upper: bool) -> String {
    let p = precision.unwrap_or(6).max(1);
    let (digits, exp10) = significant(v);
    let x = if digits.is_empty() {
        0
    } else {
        let (_, carried) = round_digits(&digits, p);
        exp10 + i32::from(carried)
    };
    let body = if x >= -4 && x < i32::try_from(p).unwrap_or(i32::MAX) {
        #[allow(clippy::cast_sign_loss)]
        let places = (i32::try_from(p).unwrap_or(i32::MAX) - 1 - x).max(0) as usize;
        fixed_form(v, places, hash)
    } else {
        scientific_form(v, p - 1, hash, upper)
    };
    if hash {
        return body;
    }
    strip_trailing_zeros(&body)
}

/// Drop `%g`'s trailing fraction zeros, and the radix point if it is left bare.
/// The exponent, if there is one, is not part of the fraction.
fn strip_trailing_zeros(body: &str) -> String {
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], &body[at..]),
        None => (body, ""),
    };
    if !mantissa.contains('.') {
        return body.to_string();
    }
    let trimmed = mantissa.trim_end_matches('0');
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    format!("{trimmed}{exponent}")
}

/// `%a`. Returns the `0x` prefix separately, because a zero-padded field puts
/// its zeros *after* it.
fn hex_form(v: ExtF80, precision: Option<usize>, hash: bool, upper: bool) -> (String, String) {
    let (m, e) = v.decompose();
    // The stored significand is 64 bits with an explicit integer bit, so it is
    // exactly sixteen hex digits and the radix point sits after the first.
    let lead = u32::try_from(m >> 60).unwrap_or(0);
    let mut rest: Vec<u8> = (0..15)
        .map(|i| hex_digit(u32::try_from((m >> (56 - i * 4)) & 0xf).unwrap_or(0), upper))
        .collect();
    let mut lead = lead;
    let mut exponent = if m == 0 { 0 } else { e + 60 };

    match precision {
        None => {
            while rest.last() == Some(&b'0') {
                rest.pop();
            }
        }
        Some(p) if p < rest.len() => {
            let kept = &rest[..p];
            let first_dropped = hex_value(rest[p]);
            let tail_nonzero = rest[p + 1..].iter().any(|&c| c != b'0');
            let last_odd = kept
                .last()
                .map_or(lead % 2 == 1, |&c| hex_value(c) % 2 == 1);
            let up = match first_dropped.cmp(&8) {
                Ordering::Greater => true,
                Ordering::Less => false,
                Ordering::Equal => tail_nonzero || last_odd,
            };
            let mut kept = kept.to_vec();
            if up {
                let mut carry = true;
                for i in (0..kept.len()).rev() {
                    let d = hex_value(kept[i]) + 1;
                    if d == 16 {
                        kept[i] = b'0';
                    } else {
                        kept[i] = hex_digit(d, upper);
                        carry = false;
                        break;
                    }
                }
                if carry {
                    lead += 1;
                    if lead == 16 {
                        // `0xf.fp+4` at precision 0 becomes `0x1p+8`, not
                        // `0x10p+4`: the carry is a new binary exponent.
                        lead = 1;
                        exponent += 4;
                    }
                }
            }
            rest = kept;
        }
        Some(p) => rest.resize(p, b'0'),
    }

    let prefix = if upper { "0X" } else { "0x" };
    let point = if rest.is_empty() && !hash { "" } else { "." };
    let mark = if upper { 'P' } else { 'p' };
    let esign = if exponent < 0 { '-' } else { '+' };
    let body = format!(
        "{}{point}{}{mark}{esign}{}",
        char::from(hex_digit(lead, upper)),
        String::from_utf8_lossy(&rest),
        exponent.abs()
    );
    (prefix.to_string(), body)
}

fn hex_digit(v: u32, upper: bool) -> u8 {
    let table: &[u8; 16] = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    table[(v & 0xf) as usize]
}

fn hex_value(c: u8) -> u32 {
    char::from(c).to_digit(16).unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn p(s: &str) -> ExtF80 {
        xstrtold(s.as_bytes()).unwrap()
    }

    fn f(fmt: &str, v: ExtF80) -> String {
        // A tiny spec parser, for tests only, so the cases below can be written
        // the way they were measured from C.
        let b = fmt.as_bytes();
        assert_eq!(b[0], b'%');
        let mut i = 1;
        let mut spec = Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: None,
            conv: b'f',
        };
        while i < b.len() {
            match b[i] {
                b'-' => spec.minus = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.hash = true,
                b'0' => spec.zero = true,
                _ => break,
            }
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i > start {
            spec.width = fmt[start..i].parse().unwrap();
        }
        if i < b.len() && b[i] == b'.' {
            i += 1;
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            spec.precision = Some(if i > start {
                fmt[start..i].parse().unwrap()
            } else {
                0
            });
        }
        if i < b.len() && b[i] == b'L' {
            i += 1;
        }
        spec.conv = b[i];
        render(&spec, v)
    }

    // ---------------- parsing ----------------

    #[test]
    fn small_integers_are_exact() {
        for v in [0u32, 1, 2, 3, 10, 12345, u32::MAX] {
            assert_eq!(f("%.0Lf", p(&v.to_string())), v.to_string(), "{v}");
        }
    }

    #[test]
    fn sixty_four_bit_integers_survive_that_f64_would_not() {
        // 2^64 - 1 needs 64 significand bits: exact here, rounded in an f64.
        assert_eq!(
            f("%.0Lf", p("18446744073709551615")),
            "18446744073709551615"
        );
        assert_eq!(f("%.0Lf", p("9007199254740993")), "9007199254740993");
    }

    #[test]
    fn the_digits_f64_loses_are_kept() {
        // The first line of the divergence quoted in the module documentation.
        assert_eq!(
            f("%.16Lf", p("145.0612310077283783")),
            "145.0612310077283783"
        );
        assert_eq!(
            f("%.17Lf", p("97.17918943860668928")),
            "97.17918943860668928"
        );
        assert_eq!(
            f("%.18Lf", p("36.395322750142177436")),
            "36.395322750142177437"
        );
    }

    #[test]
    fn leading_space_and_sign_are_accepted() {
        assert_eq!(f("%.1Lf", p("  12")), "12.0");
        assert_eq!(f("%.1Lf", p("+.5")), "0.5");
        assert_eq!(f("%.1Lf", p("5.")), "5.0");
        assert_eq!(f("%.1Lf", p("-0.5")), "-0.5");
    }

    #[test]
    fn trailing_junk_is_refused() {
        assert!(xstrtold(b"5 ").is_none());
        assert!(xstrtold(b"1_0").is_none());
        assert!(xstrtold(b"0x").is_none());
        assert!(xstrtold(b"1e").is_none());
        assert!(xstrtold(b"1e+").is_none());
        assert!(xstrtold(b".").is_none());
        assert!(xstrtold(b"").is_none());
        assert!(xstrtold(b"abc").is_none());
        assert!(xstrtold(b"--").is_none());
    }

    #[test]
    fn strtold_stops_where_glibc_stops() {
        // Measured with a C probe: the numeral ends before the malformed part.
        assert_eq!(strtold(b"1e").consumed, 1);
        assert_eq!(strtold(b"1e+").consumed, 1);
        assert_eq!(strtold(b"1.5e").consumed, 3);
        assert_eq!(strtold(b"0x").consumed, 1);
        assert_eq!(strtold(b"0x.p1").consumed, 1);
        assert_eq!(strtold(b"  12").consumed, 4);
        assert_eq!(strtold(b".").consumed, 0);
        assert_eq!(strtold(b"nanq").consumed, 3);
        assert_eq!(strtold(b"nan()").consumed, 5);
        assert_eq!(strtold(b"NaN(1x)").consumed, 7);
        assert_eq!(strtold(b"infinity").consumed, 8);
        assert_eq!(strtold(b"INFINITY").consumed, 8);
        assert_eq!(strtold(b"5.").consumed, 2);
    }

    #[test]
    fn hexadecimal_numerals_parse() {
        assert_eq!(f("%.0Lf", p("0x10")), "16");
        assert_eq!(f("%.0Lf", p("0X1P4")), "16");
        assert_eq!(f("%.1Lf", p("0x.8p1")), "1.0");
        assert_eq!(f("%.1Lf", p("0x1.p1")), "2.0");
        assert_eq!(f("%.1Lf", p("0x1p-1")), "0.5");
    }

    #[test]
    fn words_parse() {
        assert!(p("inf").is_infinite());
        assert!(!p("inf").sign_bit());
        assert!(p("-INFINITY").is_infinite());
        assert!(p("-INFINITY").sign_bit());
        assert!(p("nan").is_nan());
        assert!(p("NAN(x1)").is_nan());
    }

    #[test]
    fn the_range_boundaries_are_glibcs() {
        // Measured: a decimal that lands subnormal is a range error, one that
        // rounds up to the smallest normal is not.
        assert!(strtold(b"1e-4932").range_error);
        assert!(strtold(b"2e-4932").range_error);
        assert!(!strtold(b"3.4e-4932").range_error);
        assert!(!strtold(b"3.3621031431120935062e-4932").range_error);
        assert!(strtold(b"3.362103143112093506e-4932").range_error);
        assert!(!strtold(b"1e4932").range_error);
        assert!(strtold(b"1e4933").range_error);
        assert!(!strtold(b"1.1897e4932").range_error);
        assert!(strtold(b"1.19e4932").range_error);
        assert!(!strtold(b"0").range_error);
        assert!(!strtold(b"0e99999999999999999999").range_error);
        assert!(strtold(b"1e99999999999999999999").range_error);
        assert!(strtold(b"1e-99999999999999999999").range_error);
        // And so a subnormal operand is refused outright, while one that
        // underflows all the way to zero is accepted.
        assert!(xstrtold(b"1e-4932").is_none());
        assert!(xstrtold(b"1e-5000").is_some());
        assert!(xstrtold(b"1e4933").is_none());
    }

    #[test]
    fn zero_keeps_its_sign() {
        assert_eq!(f("%.0Lf", p("-0")), "-0");
        assert_eq!(f("%Lg", p("-0")), "-0");
        assert_eq!(f("%Le", p("-0")), "-0.000000e+00");
        assert_eq!(f("%La", p("-0")), "-0x0p+0");
        assert_eq!(f("%.0Lf", p("0")), "0");
    }

    // ---------------- arithmetic ----------------

    #[test]
    fn addition_and_multiplication_are_exact_where_they_can_be() {
        let a = p("1");
        let b = p("2");
        assert_eq!(f("%.0Lf", a.add(b)), "3");
        assert_eq!(f("%.0Lf", a.mul(b)), "2");
        let big = p("18446744073709551615");
        assert_eq!(f("%.0Lf", big.add(ExtF80::ONE)), "18446744073709551616");
    }

    #[test]
    fn a_sum_rounds_to_nearest_even_at_sixty_four_bits() {
        // 2^64 + 1 is not representable; it rounds to 2^64 (even), while
        // 2^64 + 3 rounds up to 2^64 + 4.
        let two64 = p("18446744073709551616");
        assert_eq!(f("%.0Lf", two64.add(ExtF80::ONE)), "18446744073709551616");
        assert_eq!(
            f("%.0Lf", two64.add(ExtF80::from_u32(3))),
            "18446744073709551620"
        );
    }

    #[test]
    fn cancellation_is_exact() {
        let a = p("1");
        assert!(a.add(a.neg()).is_zero());
        // x + (-x) is +0 under round-to-nearest, even for a negative x.
        assert!(!a.neg().add(a).sign_bit());
        assert!(p("-0").add(p("-0")).sign_bit());
        assert!(!p("-0").add(p("0")).sign_bit());
    }

    #[test]
    fn adding_something_far_smaller_still_rounds() {
        let one = ExtF80::ONE;
        // 2^-64 is half an ulp of 1.0, so it ties and rounds to even: 1.0.
        assert!(one.add(p("0x1p-64")).eq_value(one));
        // Anything more tips it up.
        assert!(!one.add(p("0x1.8p-64")).eq_value(one));
        // And 2^-65 is below the halfway point.
        assert!(one.add(p("0x1p-65")).eq_value(one));
    }

    #[test]
    fn infinities_and_nans_behave() {
        let inf = ExtF80::INFINITY;
        assert!(inf.add(inf).is_infinite());
        assert!(inf.add(inf.neg()).is_nan());
        assert!(inf.mul(ExtF80::ZERO).is_nan());
        assert!(inf.mul(p("2")).is_infinite());
        assert!(ExtF80::NAN.add(ExtF80::ONE).is_nan());
        assert_eq!(ExtF80::NAN.partial_cmp(ExtF80::ONE), None);
    }

    #[test]
    fn overflow_reaches_infinity() {
        let huge = p("1e4932");
        assert!(huge.mul(p("1000")).is_infinite());
    }

    #[test]
    fn comparison_orders_across_signs_and_zeros() {
        assert!(p("-1").lt(p("0")));
        assert!(p("0").lt(p("1")));
        assert!(p("-0").eq_value(p("0")));
        assert!(!p("1").lt(p("1")));
        assert!(p("1e-4000").lt(p("1e-3000")));
        assert!(p("-1e4000").lt(p("-1e3000")));
    }

    // ---------------- formatting ----------------

    #[test]
    fn fixed_rounds_half_to_even() {
        assert_eq!(f("%.0Lf", p("0.5")), "0");
        assert_eq!(f("%.0Lf", p("1.5")), "2");
        assert_eq!(f("%.0Lf", p("2.5")), "2");
        assert_eq!(f("%.0Lf", p("3.5")), "4");
        assert_eq!(f("%.0Lf", p("-0.5")), "-0");
        assert_eq!(f("%.1Lf", p("0.25")), "0.2");
        assert_eq!(f("%.1Lf", p("0.75")), "0.8");
        // 0.35 is not exactly 0.35 in binary, and lands just below the tie.
        assert_eq!(f("%.1Lf", p("0.35")), "0.3");
    }

    #[test]
    fn the_default_precision_is_six() {
        assert_eq!(f("%Lf", p("1")), "1.000000");
        assert_eq!(f("%Le", p("1")), "1.000000e+00");
        assert_eq!(f("%Lf", p("0")), "0.000000");
        assert_eq!(f("%Le", p("0")), "0.000000e+00");
    }

    #[test]
    fn a_third_matches_glibc_to_the_last_digit() {
        let third = ExtF80::ONE.mul(p("0x5.5555555555555558p-4"));
        let _ = third;
        // Computed the way `seq` would reach it is not possible without
        // division, so use the exact literal glibc prints for 1.0L/3.0L.
        let v = p("0xa.aaaaaaaaaaaaaabp-5");
        assert_eq!(f("%Lf", v), "0.333333");
        assert_eq!(f("%.20Lf", v), "0.33333333333333333334");
        assert_eq!(f("%.25Le", v), "3.3333333333333333334236835e-01");
    }

    #[test]
    fn scientific_pads_the_exponent_to_two_digits() {
        assert_eq!(f("%.2Le", p("1e-5")), "1.00e-05");
        assert_eq!(f("%.2Le", p("1e100")), "1.00e+100");
        assert_eq!(f("%.2Le", p("1e4932")), "1.00e+4932");
    }

    #[test]
    fn scientific_carries_into_the_exponent() {
        assert_eq!(f("%.0Le", p("9.9")), "1e+01");
        assert_eq!(f("%.1Le", p("9.99")), "1.0e+01");
        assert_eq!(f("%.2Le", p("999.99")), "1.00e+03");
    }

    #[test]
    fn general_picks_a_form_and_strips_zeros() {
        assert_eq!(f("%Lg", p("100000")), "100000");
        assert_eq!(f("%Lg", p("1000000")), "1e+06");
        assert_eq!(f("%Lg", p("0.0001")), "0.0001");
        assert_eq!(f("%Lg", p("0.00001")), "1e-05");
        assert_eq!(f("%Lg", p("123456789")), "1.23457e+08");
        assert_eq!(f("%.10Lg", p("1.5")), "1.5");
        assert_eq!(f("%#.10Lg", p("1.5")), "1.500000000");
        assert_eq!(f("%.3Lg", p("1234")), "1.23e+03");
        assert_eq!(f("%.1Lg", p("0.0001234")), "0.0001");
        assert_eq!(f("%.0Lg", p("1234")), "1e+03");
        assert_eq!(f("%.0Lg", p("0")), "0");
        assert_eq!(f("%Lg", p("0")), "0");
        assert_eq!(f("%#Lg", p("0")), "0.00000");
        assert_eq!(f("%#Lg", p("1")), "1.00000");
    }

    #[test]
    fn hash_keeps_a_bare_radix_point() {
        assert_eq!(f("%#.0Lf", p("1")), "1.");
        assert_eq!(f("%#.0Le", p("1")), "1.e+00");
        assert_eq!(f("%#.0Lg", p("1")), "1.");
    }

    #[test]
    fn width_and_padding_match_glibc() {
        assert_eq!(f("%20.5Lf", p("-3.14159")), "            -3.14159");
        assert_eq!(f("%-20.5Lf", p("-3.14159")), "-3.14159            ");
        assert_eq!(f("%020.5Lf", p("-3.14159")), "-0000000000003.14159");
        assert_eq!(f("%+020.5Lf", p("3.14159")), "+0000000000003.14159");
        assert_eq!(f("%08.2Lf", p("-1.5")), "-0001.50");
        assert_eq!(f("%+.2Lf", p("1.5")), "+1.50");
        assert_eq!(f("% .2Lf", p("1.5")), " 1.50");
        assert_eq!(f("%-8.2Lf", p("1.5")), "1.50    ");
    }

    #[test]
    fn uppercase_conversions_are_uppercase() {
        assert_eq!(f("%LF", p("1")), "1.000000");
        assert_eq!(f("%LE", p("1")), "1.000000E+00");
        assert_eq!(f("%LG", p("1e20")), "1E+20");
        assert_eq!(f("%LA", p("1")), "0X8P-3");
    }

    #[test]
    fn hex_form_shows_the_explicit_integer_bit() {
        // Measured from glibc: the leading digit is the top four bits of a
        // 64-bit significand, so 1.0 is 0x8p-3 rather than 0x1p+0.
        assert_eq!(f("%La", p("1")), "0x8p-3");
        assert_eq!(f("%La", p("0.5")), "0x8p-4");
        assert_eq!(f("%La", p("3")), "0xcp-2");
        assert_eq!(f("%La", p("1.5")), "0xcp-3");
        assert_eq!(f("%La", p("255")), "0xf.fp+4");
        assert_eq!(f("%La", p("0")), "0x0p+0");
        assert_eq!(f("%.3La", p("1")), "0x8.000p-3");
        assert_eq!(f("%.0La", p("1")), "0x8p-3");
        assert_eq!(f("%#.0La", p("1")), "0x8.p-3");
        assert_eq!(f("%#La", p("1")), "0x8.p-3");
        assert_eq!(f("%+La", p("1")), "+0x8p-3");
        assert_eq!(f("%015La", p("1")), "0x0000000008p-3");
        assert_eq!(f("%015La", p("255")), "0x0000000f.fp+4");
    }

    #[test]
    fn hex_form_rounds_and_can_carry_into_the_exponent() {
        assert_eq!(f("%.0La", p("255")), "0x1p+8");
        let tenth = p("0.1");
        assert_eq!(f("%La", tenth), "0xc.ccccccccccccccdp-7");
        assert_eq!(f("%.0La", tenth), "0xdp-7");
        assert_eq!(f("%.1La", tenth), "0xc.dp-7");
        assert_eq!(f("%.2La", tenth), "0xc.cdp-7");
        assert_eq!(f("%.20La", tenth), "0xc.ccccccccccccccd00000p-7");
        let e4 = p("1e-4");
        assert_eq!(f("%La", e4), "0xd.1b71758e219652cp-17");
        assert_eq!(f("%.1La", e4), "0xd.2p-17");
        assert_eq!(f("%.2La", e4), "0xd.1bp-17");
    }

    #[test]
    fn non_finite_values_format_as_words() {
        let inf = ExtF80::INFINITY;
        assert_eq!(f("%Lf", inf), "inf");
        assert_eq!(f("%Le", inf), "inf");
        assert_eq!(f("%Lg", inf), "inf");
        assert_eq!(f("%La", inf), "inf");
        assert_eq!(f("%LF", inf), "INF");
        assert_eq!(f("%LA", inf), "INF");
        assert_eq!(f("%Lf", inf.neg()), "-inf");
        assert_eq!(f("%10Lf", inf), "       inf");
        assert_eq!(f("%-10Lf", inf), "inf       ");
        assert_eq!(f("%010Lf", inf), "       inf");
        assert_eq!(f("%+Lf", inf), "+inf");
        assert_eq!(f("%Lf", ExtF80::NAN), "nan");
        assert_eq!(f("%Lf", ExtF80::NAN.neg()), "-nan");
    }

    #[test]
    fn very_small_and_very_large_values_expand_exactly() {
        assert_eq!(
            f("%.40Lf", p("1e-30")),
            "0.0000000000000000000000000000010000000000"
        );
        // 1e30 is 2^30 * 5^30, and 5^30 needs 70 bits, so the value is *not*
        // representable and `%.0Lf` shows where it actually lands. Measured:
        // `printf("%.0Lf", 1e30L)` on glibc prints exactly this.
        assert_eq!(f("%.0Lf", p("1e30")), "1000000000000000000024696061952");
        // 1e20 is 2^20 * 5^20 and 5^20 needs 47 bits, so that one is exact.
        assert_eq!(f("%.0Lf", p("1e20")), "100000000000000000000");
    }

    #[test]
    fn a_round_trip_through_text_is_stable() {
        // Whatever `%.20Le` writes, reading it back must give the same value —
        // 21 significant digits is more than 64 bits needs.
        for s in ["1", "0.1", "3.14159265358979323846", "1e-300", "6.02e23"] {
            let v = p(s);
            let text = f("%.20Le", v);
            assert!(p(&text).eq_value(v), "{s} -> {text}");
        }
    }
}
